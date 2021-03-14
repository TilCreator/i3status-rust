use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
    cmp::max,
};

use crossbeam_channel::Sender;
use serde_derive::Deserialize;

use crate::blocks::{Block, ConfigBlock, Update};
use crate::config::SharedConfig;
use crate::de::deserialize_duration;
use crate::errors::*;
use crate::input::I3BarEvent;
use crate::scheduler::Task;
use crate::widgets::text::TextWidget;
use crate::widgets::{I3BarWidget, Spacing, State};
use crate::formatting::{value::Value, FormatTemplate};

#[derive(Clone)]
pub struct Radeontop {
    id: usize,
    update_interval: Duration,
    gpu_info: usize,
    gpu_warning: usize,
    gpu_critical: usize,
    vram_info: usize,
    vram_warning: usize,
    vram_critical: usize,
    format: FormatTemplate,

    text: TextWidget,
    last_update: Arc<Mutex<Option<RadeontopDataDump>>>,

    shared_config: SharedConfig,
}

#[derive(Deserialize, Debug, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct RadeontopConfig {
    /// Update interval in seconds
    #[serde(
        default = "RadeontopConfig::default_interval",
        deserialize_with = "deserialize_duration"
    )]
    pub interval: Duration,

    /// Sets the `--mem` flag for radeontop
    #[serde(default = "RadeontopConfig::default_mem")]
    pub mem: bool,

    /// Sets the `--bus` arg for radeontop
    #[serde(default = "RadeontopConfig::default_bus")]
    pub bus: Option<String>,

    /// Sets the `--path` arg for radeontop
    #[serde(default = "RadeontopConfig::default_path")]
    pub path: Option<String>,

    /// Sets the `--ticks` arg for radeontop
    #[serde(default = "RadeontopConfig::default_ticks")]
    pub ticks: Option<String>,

    /// Minimum gpu usage, where state is set to info
    #[serde(default = "RadeontopConfig::default_gpu_info")]
    pub gpu_info: usize,

    /// Minimum gpu usage, where state is set to warning
    #[serde(default = "RadeontopConfig::default_gpu_warning")]
    pub gpu_warning: usize,

    /// Minimum gpu usage, where state is set to critical
    #[serde(default = "RadeontopConfig::default_gpu_critical")]
    pub gpu_critical: usize,

    /// Minimum vram usage, where state is set to info
    #[serde(default = "RadeontopConfig::default_vram_info")]
    pub vram_info: usize,

    /// Minimum vram usage, where state is set to warning
    #[serde(default = "RadeontopConfig::default_vram_warning")]
    pub vram_warning: usize,

    /// Minimum vram usage, where state is set to critical
    #[serde(default = "RadeontopConfig::default_vram_critical")]
    pub vram_critical: usize,

    /// Format override
    #[serde(default = "RadeontopConfig::default_format")]
    pub format: String,
}

impl RadeontopConfig {
    fn default_interval() -> Duration {
        Duration::from_secs(1)
    }

    fn default_mem() -> bool {
        false
    }

    fn default_bus() -> Option<String> {
        None
    }

    fn default_path() -> Option<String> {
        None
    }

    fn default_ticks() -> Option<String> {
        None
    }

    fn default_gpu_info() -> usize {
        30
    }

    fn default_gpu_warning() -> usize {
        60
    }

    fn default_gpu_critical() -> usize {
        90
    }

    fn default_vram_info() -> usize {
        30
    }

    fn default_vram_warning() -> usize {
        60
    }

    fn default_vram_critical() -> usize {
        90
    }

    fn default_format() -> String {
        "{gpu} {vram_used_percentage}".into()
    }
}

impl ConfigBlock for Radeontop {
    type Config = RadeontopConfig;

    fn new(
        id: usize,
        block_config: Self::Config,
        shared_config: SharedConfig,
        tx_update_request: Sender<Task>,
    ) -> Result<Self> {
        let last_update = Arc::new(Mutex::new(None));

        {
            let args = {
                let mut args: Vec<String> = vec![
                    "--dump-interval".into(),
                    block_config.interval.as_secs().to_string(),
                    "--dump-format".into(),
                    "json".into(),
                    "--dump".into(),
                    "-".into(),
                ];
                eprintln!("{:?}", args);
                if block_config.mem {
                    args.push("--mem".into());
                }
                if let Some(bus) = block_config.bus {
                    args.append(&mut vec!["--bus".into(), bus]);
                }
                if let Some(path) = block_config.path {
                    args.append(&mut vec!["--path".into(), path]);
                }
                if let Some(ticks) = block_config.ticks {
                    args.append(&mut vec!["--ticks".into(), ticks]);
                }
                args
            };
            let last_update = last_update.clone();
            let tx_update_request = tx_update_request.clone();
            thread::Builder::new()
                .name("radeontop".into())
                .spawn(move || {
                    let mut radeontop_process = Command::new("radeontop")
                        .args(args)
                        .stdout(Stdio::piped())
                        .spawn()
                        .expect("radeontop failed");
                    let lines = BufReader::new(radeontop_process.stdout.take().unwrap())
                        .lines()
                        .map(|l| serde_json::from_str::<RadeontopDataDump>(&l.unwrap()).unwrap());

                    for line in lines {
                        *{ last_update.lock().unwrap() } = Some(line);

                        tx_update_request
                            .send(Task {
                                id: id,
                                update_time: Instant::now(),
                            })
                            .unwrap();
                    }

                    panic!("radeontop died");
                })
                .unwrap();
        }

        Ok(Radeontop {
            id,
            update_interval: block_config.interval,
            text: TextWidget::new(id, 0, shared_config.clone())
                .with_icon("gpu")?
                .with_spacing(Spacing::Inline),
            shared_config,
            last_update: last_update,
            gpu_info: block_config.gpu_info,
            gpu_warning: block_config.gpu_warning,
            gpu_critical: block_config.gpu_critical,
            vram_info: block_config.vram_info,
            vram_warning: block_config.vram_warning,
            vram_critical: block_config.vram_critical,
            format: FormatTemplate::from_string(&block_config.format)
                .block_error("radeontop", "Invalid format specified for radeontop")?,
        })
    }
}

impl Block for Radeontop {
    fn update(&mut self) -> Result<Option<Update>> {
        if let Some(last_update) = self.last_update.lock().unwrap().as_ref() {
            self.text.set_state(match {
                max(match (last_update.vram.used_percentage * 100f64).round() as usize {
                    x if x > self.vram_critical => 3,
                    x if x > self.vram_warning => 2,
                    x if x > self.vram_info => 1,
                    _ => 0,
                }, match (last_update.gpu * 100f64).round() as usize {
                    x if x > self.gpu_critical => 3,
                    x if x > self.gpu_warning => 2,
                    x if x > self.gpu_info => 1,
                    _ => 0,
                })
            } {
                3 => State::Critical,
                2 => State::Warning,
                1 => State::Info,
                0 => State::Idle,
                _ => unreachable!(),
            });

            let values = map!(
                "bus" => Value::from_string(format!("{:02x}", last_update.bus)),
                "gpu" => Value::from_float(last_update.gpu * 100f64).percents(),
                "ee" => Value::from_float(last_update.ee * 100f64).percents(),
                "vgt" => Value::from_float(last_update.vgt * 100f64).percents(),
                "ta" => Value::from_float(last_update.ta * 100f64).percents(),
                "tc" => Value::from_float(last_update.tc * 100f64).percents(),
                "sx" => Value::from_float(last_update.sx * 100f64).percents(),
                "sh" => Value::from_float(last_update.sh * 100f64).percents(),
                "spi" => Value::from_float(last_update.spi * 100f64).percents(),
                "smx" => Value::from_float(last_update.smx * 100f64).percents(),
                "sc" => Value::from_float(last_update.sc * 100f64).percents(),
                "pa" => Value::from_float(last_update.pa * 100f64).percents(),
                "db" => Value::from_float(last_update.db * 100f64).percents(),
                "cb" => Value::from_float(last_update.cb * 100f64).percents(),
                "cr" => Value::from_float(last_update.cr * 100f64).percents(),

                "vram_used_percentage" => Value::from_float(last_update.vram.used_percentage * 100f64).percents(),
                "vram_used_bytes" => Value::from_integer(last_update.vram.used_bytes as i64).bytes(),
                "vram_max_bytes" => Value::from_integer(last_update.vram.max_bytes as i64).bytes(),

                "gtt_used_percentage" => Value::from_float(last_update.gtt.used_percentage * 100f64).percents(),
                "gtt_used_bytes" => Value::from_integer(last_update.gtt.used_bytes as i64).bytes(),
                "gtt_max_bytes" => Value::from_integer(last_update.gtt.max_bytes as i64).bytes(),

                "mclk_used_percentage" => Value::from_float(last_update.mclk.used_percentage * 100f64).percents(),
                "mclk_used_hz" => Value::from_float(last_update.mclk.used_hz).hertz(),
                "mclk_max_hz" => Value::from_float(last_update.mclk.max_hz).hertz(),

                "sclk_used_percentage" => Value::from_float(last_update.sclk.used_percentage * 100f64).percents(),
                "sclk_used_hz" => Value::from_float(last_update.sclk.used_hz).hertz(),
                "sclk_max_hz" => Value::from_float(last_update.sclk.max_hz).hertz(),
                );
            self.text
                .set_text(self.format.render(&values)?);
        }

        Ok(Some(Update::Once))
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }

    fn click(&mut self, _: &I3BarEvent) -> Result<()> {
        Ok(())
    }

    fn id(&self) -> usize {
        self.id
    }
}

#[derive(Deserialize, Debug)]
struct RadeontopDataDump {
    timestamp: f64,
    bus: usize,
    /// Graphics pipe: percent
    gpu: f64,
    /// Event Engine: percent
    ee: f64,
    /// Vertex Grouper + Tesselator: percent
    vgt: f64,
    /// Texture Addresser: percent
    ta: f64,
    /// Texture Cache: percent
    #[serde(default = "RadeontopDataDump::default_0f64")]
    tc: f64,
    /// Shader Export: percent
    sx: f64,
    /// Sequencer Instruction Cache: percent
    sh: f64,
    /// Shader Interpolator: percent
    spi: f64,
    /// Shader Memory Exchange: percent
    #[serde(default = "RadeontopDataDump::default_0f64")]
    smx: f64,
    /// Scan Converter: percent
    sc: f64,
    /// Primitive Assembly: percent
    pa: f64,
    /// Depth Block: percent
    db: f64,
    /// Color Block: percent
    cb: f64,
    /// Clip Rectangle: percent
    #[serde(default = "RadeontopDataDump::default_0f64")]
    cr: f64,
    /// Video Ram
    #[serde(default = "Ram::default")]
    vram: Ram,
    /// Graphics Translation Table
    #[serde(default = "Ram::default")]
    gtt: Ram,
    /// Memory Clock
    #[serde(default = "Clock::default")]
    mclk: Clock,
    /// Shader Clock
    #[serde(default = "Clock::default")]
    sclk: Clock,
}

impl RadeontopDataDump {
    fn default_0f64() -> f64 {
        0f64
    }
}

#[derive(Deserialize, Debug)]
struct Ram {
    #[serde(rename = "used_per")]
    used_percentage: f64,
    #[serde(rename = "used_b")]
    used_bytes: usize,
    #[serde(rename = "max_b")]
    max_bytes: usize,
}

impl Default for Ram {
    fn default() -> Self {
        Self {
            used_percentage: 0f64,
            used_bytes: 0,
            max_bytes: 0,
        }
    }
}

#[derive(Deserialize, Debug)]
struct Clock {
    #[serde(rename = "used_per")]
    used_percentage: f64,
    used_hz: f64,
    max_hz: f64,
}

impl Default for Clock {
    fn default() -> Self {
        Self {
            used_percentage: 0f64,
            used_hz: 0f64,
            max_hz: 0f64,
        }
    }
}
