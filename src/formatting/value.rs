use super::Variable;
use crate::errors::*;

#[derive(Debug, PartialEq, Clone)]
pub enum Unit {
    BitsPerSecond,
    BytesPerSecond,
    Percents,
    Degrees,
    Seconds,
    Watts,
    Hertz,
    Bytes,
    None,
    Other(String), //TODO: do not allow custom units?
}

#[derive(Debug, Clone)]
pub enum Suffix {
    Nano,
    Micro,
    Milli,
    One,
    Kilo,
    Mega,
    Giga,
    Tera,
}

#[derive(Debug, Clone)]
pub struct Value {
    unit: Unit,
    min_width: usize,
    icon: Option<String>,
    value: InternalValue,
}

#[derive(Debug, Clone)]
enum InternalValue {
    Text(String),
    Integer(i64),
    Float(f64),
}

impl Unit {
    pub fn from_string(s: &str) -> Self {
        match s {
            "Bi/s" => Self::BitsPerSecond,
            "B/s" => Self::BytesPerSecond,
            "%" => Self::Percents,
            "°" => Self::Degrees,
            "s" => Self::Seconds,
            "W" => Self::Watts,
            "Hz" => Self::Hertz,
            "B" => Self::Bytes,
            "" => Self::None,
            x => Self::Other(x.to_string()),
        }
    }

    pub fn to_string(&self) -> String {
        String::from(match self {
            Self::BitsPerSecond => "Bi/s",
            Self::BytesPerSecond => "B/s",
            Self::Percents => "%",
            Self::Degrees => "°",
            Self::Seconds => "s",
            Self::Watts => "W",
            Self::Hertz => "Hz",
            Self::Bytes => "B",
            Self::None => "",
            Self::Other(unit) => unit.as_str(),
        })
    }
}

impl Suffix {
    pub fn from_string(s: &str) -> Result<Self> {
        match s {
            "n" => Ok(Self::Nano),
            "u" => Ok(Self::Micro),
            "m" => Ok(Self::Milli),
            "1" => Ok(Self::One),
            "K" => Ok(Self::Kilo),
            "M" => Ok(Self::Mega),
            "G" => Ok(Self::Giga),
            "T" => Ok(Self::Tera),
            x => Err(ConfigurationError(
                "Can not parse suffix".to_string(),
                format!("unknown suffix: '{}'", x.to_string()),
            )),
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Self::Nano => "n".to_string(),
            Self::Micro => "u".to_string(),
            Self::Milli => "m".to_string(),
            Self::One => "".to_string(),
            Self::Kilo => "K".to_string(),
            Self::Mega => "M".to_string(),
            Self::Giga => "G".to_string(),
            Self::Tera => "T".to_string(),
        }
    }
}

//FIXME: fix confvertation of bytes (2^10 != 10^3)
//FIXME: do not use suffixes smaller than `One` for bytes
fn format_number(raw_value: f64, min_width: usize, min_suffix: &Suffix) -> String {
    let min_exp_level = match min_suffix {
        Suffix::Tera => 4,
        Suffix::Giga => 3,
        Suffix::Mega => 2,
        Suffix::Kilo => 1,
        Suffix::One => 0,
        Suffix::Milli => -1,
        Suffix::Micro => -2,
        Suffix::Nano => -3,
    };

    let exp_level = (raw_value.log10().div_euclid(3.) as i32).clamp(min_exp_level, 4);
    let value = raw_value / (10f64).powi(exp_level * 3);

    let suffix = match exp_level {
        4 => Suffix::Tera,
        3 => Suffix::Giga,
        2 => Suffix::Mega,
        1 => Suffix::Kilo,
        0 => Suffix::One,
        -1 => Suffix::Milli,
        -2 => Suffix::Micro,
        _ => Suffix::Nano,
    };

    // The length of the integer part of a number
    let digits = (value.log10().floor() + 1.0).max(1.0) as isize;
    // How many characters is left for "." and the fractional part?
    match min_width as isize - digits {
        // No characters left
        x if x <= 0 => format!("{:.0}{}", value, suffix.to_string()),
        // Only one character -> print a trailing dot
        x if x == 1 => format!("{:.0}{}.", value, suffix.to_string()),
        // There is space for fractional part
        rest => format!("{:.*}{}", (rest as usize) - 1, value, suffix.to_string()),
    }
}

impl Value {
    // Constuctors
    pub fn from_string(text: String) -> Self {
        Self {
            icon: None,
            min_width: 0,
            unit: Unit::None,
            value: InternalValue::Text(text),
        }
    }
    pub fn from_integer(value: i64) -> Self {
        Self {
            icon: None,
            min_width: 2,
            unit: Unit::None,
            value: InternalValue::Integer(value),
        }
    }
    pub fn from_float(value: f64) -> Self {
        Self {
            icon: None,
            min_width: 3,
            unit: Unit::None,
            value: InternalValue::Float(value),
        }
    }

    // Set options
    pub fn icon(mut self, icon: String) -> Self {
        self.icon = Some(icon);
        self
    }
    //pub fn min_width(mut self, min_width: usize) -> Self {
    //self.min_width = min_width;
    //self
    //}

    // Units
    pub fn degrees(mut self) -> Self {
        self.unit = Unit::Degrees;
        self
    }
    pub fn percents(mut self) -> Self {
        self.unit = Unit::Percents;
        self
    }
    pub fn bits_per_second(mut self) -> Self {
        self.unit = Unit::BitsPerSecond;
        self
    }
    pub fn bytes_per_second(mut self) -> Self {
        self.unit = Unit::BytesPerSecond;
        self
    }
    pub fn seconds(mut self) -> Self {
        self.unit = Unit::Seconds;
        self
    }
    pub fn watts(mut self) -> Self {
        self.unit = Unit::Watts;
        self
    }
    pub fn hertz(mut self) -> Self {
        self.unit = Unit::Hertz;
        self
    }
    pub fn bytes(mut self) -> Self {
        self.unit = Unit::Bytes;
        self
    }

    pub fn format(&self, var: &Variable) -> String {
        let min_width = var.min_width.unwrap_or(self.min_width);
        let pad_with = var.pad_with.unwrap_or(' ');
        let unit = var.unit.as_ref().unwrap_or(&self.unit);

        let value = match self.value {
            InternalValue::Text(ref text) => {
                let mut text = text.clone();
                let text_len = text.len();
                if text_len < min_width {
                    for _ in text_len..min_width {
                        text.push(pad_with);
                    }
                }
                text
            }
            InternalValue::Integer(value) => {
                //TODO better way to do it?
                let value = if self.unit == Unit::BytesPerSecond && *unit == Unit::BitsPerSecond {
                    value * 8
                } else if self.unit == Unit::BitsPerSecond && *unit == Unit::BytesPerSecond {
                    value / 8
                } else {
                    value
                };

                let text = value.to_string();
                let mut retval = String::new();
                let text_len = text.len();
                if text_len < min_width {
                    for _ in text_len..min_width {
                        retval.push(pad_with);
                    }
                }
                retval.push_str(&text);
                retval
            }
            InternalValue::Float(value) => {
                //TODO better way to do it?
                let value = if self.unit == Unit::BytesPerSecond && *unit == Unit::BitsPerSecond {
                    value * 8.
                } else if self.unit == Unit::BitsPerSecond && *unit == Unit::BytesPerSecond {
                    value / 8.
                } else {
                    value
                };

                format_number(
                    value,
                    min_width,
                    var.min_suffix.as_ref().unwrap_or(&Suffix::Nano),
                )
            }
        };
        if let Some(ref icon) = self.icon {
            format!("{}{}{}", icon, value, unit.to_string())
        } else {
            format!("{}{}", value, unit.to_string())
        }
    }
}