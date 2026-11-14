use crate::Reading;

/// Formats readings as a Markdown table.
pub fn to_markdown(readings: &[Reading]) -> String {
    let mut out = String::from("| Sensor | Value |\n| --- | ---: |\n");
    for reading in readings {
        out.push_str(&format!("| {} | {:.1} |\n", reading.sensor, reading.value));
    }
    out
}
