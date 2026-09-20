use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::diag;
use crate::metrics::pawnio::CpuTemp;

/// CPU temperature.
///
/// Primary source is the hardware itself, read straight through the PawnIO
/// kernel driver (no other application needs to run). When PawnIO is not
/// available (driver missing, or an unsupported CPU vendor), it falls back to
/// LibreHardwareMonitor's HTTP server.
///
/// Windows has no reliable public user-mode CPU temperature API, so the driver
/// route is the only way to avoid depending on a running helper app.
pub struct TempCollector {
    port: u16,
    cpu: CpuTemp,
    cached: Option<f32>,
    ticks: u32,
    cooldown: u32,
    source: Option<&'static str>,
}

#[derive(Deserialize, Default)]
struct Node {
    #[serde(rename = "Text", default)]
    text: String,
    #[serde(rename = "SensorId", default)]
    sensor_id: String,
    #[serde(rename = "Type", default)]
    sensor_type: String,
    #[serde(rename = "RawValue", default)]
    raw_value: Option<Value>,
    #[serde(rename = "Children", default)]
    children: Vec<Node>,
}

/// Query LHM every this many samples.
const QUERY_INTERVAL_TICKS: u32 = 3;
/// After a failed query, skip this many samples before trying again.
const FAILURE_COOLDOWN_TICKS: u32 = 10;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
const IO_TIMEOUT: Duration = Duration::from_secs(2);

impl TempCollector {
    pub fn new(port: u16) -> Self {
        let cpu = CpuTemp::new();
        diag::log(&format!("temperature backend: {}", cpu.describe()));
        Self {
            port,
            cpu,
            cached: None,
            ticks: QUERY_INTERVAL_TICKS,
            cooldown: 0,
            source: None,
        }
    }

    fn set_source(&mut self, source: &'static str) {
        if self.source != Some(source) {
            self.source = Some(source);
            diag::log(&format!("temperature source: {source}"));
        }
    }

    pub fn sample(&mut self) -> Option<f32> {
        // Hardware first: no other application involved.
        if let Some(temperature) = self.cpu.sample() {
            self.set_source("PawnIO");
            self.cached = Some(temperature);
            self.cooldown = 0;
            return Some(temperature);
        }

        // Fallback: LibreHardwareMonitor's HTTP server.
        self.sample_lhm()
    }

    fn sample_lhm(&mut self) -> Option<f32> {
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return self.cached;
        }

        self.ticks = self.ticks.saturating_add(1);
        if self.ticks < QUERY_INTERVAL_TICKS {
            return self.cached;
        }
        self.ticks = 0;

        match fetch_json(self.port).and_then(|json| cpu_temp_from_json(&json)) {
            Some(temp) => {
                self.set_source("LHM");
                self.cached = Some(temp);
            }
            None => {
                self.cached = None;
                self.cooldown = FAILURE_COOLDOWN_TICKS;
            }
        }
        self.cached
    }
}

fn fetch_json(port: u16) -> Option<String> {
    let addr: SocketAddr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).ok()?;
    stream.set_read_timeout(Some(IO_TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;

    let request = format!(
        "GET /data.json HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;

    let mut raw = Vec::with_capacity(128 * 1024);
    stream.read_to_end(&mut raw).ok()?;

    // LHM sends a plain HTTP/1.1 response with Content-Length, so the body
    // starts right after the header terminator.
    let split = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    String::from_utf8(raw[split + 4..].to_vec()).ok()
}

fn cpu_temp_from_json(json: &str) -> Option<f32> {
    let root: Node = serde_json::from_str(json).ok()?;
    let mut best = None;
    let mut fallback = None;
    collect(&root, &mut best, &mut fallback);
    best.or(fallback)
}

fn collect(node: &Node, best: &mut Option<f32>, fallback: &mut Option<f32>) {
    if node.sensor_type == "Temperature"
        && is_cpu(&node.sensor_id)
        && let Some(value) = node.raw_value.as_ref().and_then(raw_number)
        && value.is_finite()
        && value > 0.0
    {
        let value = value as f32;
        let label = node.text.as_str();
        let preferred =
            label.contains("Tctl") || label.contains("Tdie") || label.contains("Package");
        if preferred && best.is_none() {
            *best = Some(value);
        } else if !preferred && fallback.is_none() {
            *fallback = Some(value);
        }
    }
    for child in &node.children {
        collect(child, best, fallback);
    }
}

/// LHM serializes sensor values as unit-suffixed strings, e.g. `"56.9 °C"`.
/// Numbers are accepted too, in case a future version changes that.
fn raw_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.split_whitespace().next()?.parse().ok(),
        _ => None,
    }
}

fn is_cpu(sensor_id: &str) -> bool {
    sensor_id.starts_with("/intelcpu") || sensor_id.starts_with("/amdcpu") || sensor_id.starts_with("/cpu")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires a running LibreHardwareMonitor with the web server enabled"]
    fn debug_live_lhm() {
        let json = fetch_json(8085).expect("fetch_json failed");
        eprintln!("json length = {}", json.len());
        eprintln!("cpu temp = {:?}", cpu_temp_from_json(&json));
    }

    #[test]
    fn parses_unit_suffixed_values() {
        let json = r#"{
            "Text": "Sensor",
            "Children": [
                {
                    "Text": "Core (Tctl/Tdie)",
                    "SensorId": "/amdcpu/0/temperature/2",
                    "Type": "Temperature",
                    "RawValue": "56.9 \u00B0C",
                    "Children": []
                }
            ]
        }"#;
        assert_eq!(cpu_temp_from_json(json), Some(56.9));
    }
}
