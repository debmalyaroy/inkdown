use std::collections::HashMap;

/// A reading taken from one sensor.
#[derive(Debug, Clone)]
struct Reading {
    sensor: String,
    value: f64,
}

fn average(readings: &[Reading]) -> HashMap<String, f64> {
    let mut total: HashMap<String, (f64, u32)> = HashMap::new();
    for reading in readings {
        let entry = total.entry(reading.sensor.clone()).or_insert((0.0, 0));
        entry.0 += reading.value;
        entry.1 += 1;
    }
    total.into_iter().map(|(name, (sum, n))| (name, sum / n as f64)).collect()
}

fn main() {
    let readings = vec![
        Reading { sensor: "north".into(), value: 21.5 },
        Reading { sensor: "south".into(), value: 24.0 },
        Reading { sensor: "north".into(), value: 22.1 },
    ];
    for (sensor, mean) in average(&readings) {
        println!("{sensor}: {mean:.1}");
    }
}
