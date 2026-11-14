//! Reader for Apache Avro object container files (`.avro`).
//!
//! The file carries its own schema, so it can be decoded without anything else. Records are
//! turned into JSON values for the tree view. Codecs: `null`, `deflate` and `snappy`.

use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::io::Read;

const MAGIC: &[u8; 4] = b"Obj\x01";

pub fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// The result of reading a container file.
#[derive(Debug)]
pub struct File {
    pub schema: Value,
    pub codec: String,
    /// Records decoded (at most the limit asked for).
    pub records: Vec<Value>,
    /// Records in the file, counted from the block headers.
    pub total: u64,
}

impl File {
    pub fn truncated(&self) -> bool {
        (self.records.len() as u64) < self.total
    }

    /// One JSON document with the file facts, the schema and the records.
    pub fn to_json_text(&self) -> String {
        let mut info = Map::new();
        info.insert("codec".into(), Value::String(self.codec.clone()));
        info.insert("records".into(), Value::Number(self.total.into()));
        if self.truncated() {
            info.insert("shown".into(), Value::Number((self.records.len() as u64).into()));
        }
        let mut root = Map::new();
        root.insert("avro".into(), Value::Object(info));
        root.insert("schema".into(), self.schema.clone());
        root.insert("records".into(), Value::Array(self.records.clone()));
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ schema

#[derive(Debug, Clone)]
enum Logical {
    Date,
    TimeMillis,
    TimeMicros,
    TimestampMillis,
    TimestampMicros,
    Decimal(u32),
}

#[derive(Debug, Clone)]
enum Schema {
    Null,
    Boolean,
    Int,
    Long,
    Float,
    Double,
    Bytes,
    String,
    Record(Vec<(String, Schema)>),
    Enum(Vec<String>),
    Array(Box<Schema>),
    Map(Box<Schema>),
    Union(Vec<Schema>),
    Fixed(usize),
    /// A named type, looked up when decoding (this is what allows recursive records).
    Named(Vec<String>),
    Logical(Box<Schema>, Logical),
}

#[derive(Default)]
struct Names(HashMap<String, Schema>);

fn full_name(name: &str, namespace: &str) -> String {
    if name.contains('.') || namespace.is_empty() {
        name.to_string()
    } else {
        format!("{namespace}.{name}")
    }
}

fn primitive(name: &str) -> Option<Schema> {
    Some(match name {
        "null" => Schema::Null,
        "boolean" => Schema::Boolean,
        "int" => Schema::Int,
        "long" => Schema::Long,
        "float" => Schema::Float,
        "double" => Schema::Double,
        "bytes" => Schema::Bytes,
        "string" => Schema::String,
        _ => return None,
    })
}

fn parse_schema(v: &Value, namespace: &str, names: &mut Names, depth: usize) -> Result<Schema, String> {
    if depth > 64 {
        return Err("The Avro schema is nested too deeply.".into());
    }
    match v {
        Value::String(s) => Ok(primitive(s).unwrap_or_else(|| Schema::Named(vec![full_name(s, namespace), s.clone()]))),
        Value::Array(branches) => Ok(Schema::Union(branches.iter().map(|b| parse_schema(b, namespace, names, depth + 1)).collect::<Result<_, _>>()?)),
        Value::Object(o) => {
            let ty = o.get("type").ok_or("An Avro schema object has no \"type\".")?;
            let Some(ty) = ty.as_str() else {
                // {"type": {...}} or {"type": [...]}
                return parse_schema(ty, namespace, names, depth + 1);
            };
            let name = o.get("name").and_then(Value::as_str).unwrap_or("");
            let ns = match (name.rsplit_once('.'), o.get("namespace").and_then(Value::as_str)) {
                (Some((prefix, _)), _) => prefix.to_string(),
                (None, Some(n)) => n.to_string(),
                (None, None) => namespace.to_string(),
            };
            let full = full_name(name, &ns);
            let base = match ty {
                "record" | "error" => {
                    // register the name first, so that fields can refer to the record itself
                    names.0.insert(full.clone(), Schema::Null);
                    let mut fields = Vec::new();
                    for f in o.get("fields").and_then(Value::as_array).ok_or("An Avro record has no \"fields\".")? {
                        let fname = f.get("name").and_then(Value::as_str).ok_or("An Avro field has no name.")?;
                        let fty = f.get("type").ok_or_else(|| format!("Avro field \"{fname}\" has no type."))?;
                        fields.push((fname.to_string(), parse_schema(fty, &ns, names, depth + 1)?));
                    }
                    names.0.insert(full.clone(), Schema::Record(fields));
                    return Ok(Schema::Named(vec![full]));
                }
                "enum" => {
                    let symbols = o.get("symbols").and_then(Value::as_array).ok_or("An Avro enum has no \"symbols\".")?;
                    names.0.insert(full.clone(), Schema::Enum(symbols.iter().map(|s| s.as_str().unwrap_or("").to_string()).collect()));
                    return Ok(Schema::Named(vec![full]));
                }
                "fixed" => {
                    let size = o.get("size").and_then(Value::as_u64).ok_or("An Avro fixed type has no \"size\".")? as usize;
                    let fixed = with_logical(Schema::Fixed(size), o);
                    names.0.insert(full.clone(), fixed);
                    return Ok(Schema::Named(vec![full]));
                }
                "array" => Schema::Array(Box::new(parse_schema(o.get("items").ok_or("An Avro array has no \"items\".")?, namespace, names, depth + 1)?)),
                "map" => Schema::Map(Box::new(parse_schema(o.get("values").ok_or("An Avro map has no \"values\".")?, namespace, names, depth + 1)?)),
                other => primitive(other).unwrap_or_else(|| Schema::Named(vec![full_name(other, namespace), other.to_string()])),
            };
            Ok(with_logical(base, o))
        }
        _ => Err("The Avro schema is not valid.".into()),
    }
}

fn with_logical(base: Schema, o: &Map<String, Value>) -> Schema {
    let logical = match (o.get("logicalType").and_then(Value::as_str), &base) {
        (Some("date"), Schema::Int) => Logical::Date,
        (Some("time-millis"), Schema::Int) => Logical::TimeMillis,
        (Some("time-micros"), Schema::Long) => Logical::TimeMicros,
        (Some("timestamp-millis" | "local-timestamp-millis"), Schema::Long) => Logical::TimestampMillis,
        (Some("timestamp-micros" | "local-timestamp-micros"), Schema::Long) => Logical::TimestampMicros,
        (Some("decimal"), Schema::Bytes | Schema::Fixed(_)) => Logical::Decimal(o.get("scale").and_then(Value::as_u64).unwrap_or(0) as u32),
        _ => return base,
    };
    Schema::Logical(Box::new(base), logical)
}

// ------------------------------------------------------------------ reading

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn done(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len()).ok_or("The Avro file ends unexpectedly.")?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    /// A zig-zag encoded variable-length integer.
    fn long(&mut self) -> Result<i64, String> {
        let mut value: u64 = 0;
        let mut shift = 0;
        loop {
            let b = *self.take(1)?.first().unwrap();
            value |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err("The Avro file contains a number that is too long.".into());
            }
        }
        Ok((value >> 1) as i64 ^ -((value & 1) as i64))
    }

    fn len(&mut self) -> Result<usize, String> {
        usize::try_from(self.long()?).map_err(|_| "The Avro file contains a negative length.".to_string())
    }

    fn bytes(&mut self) -> Result<&'a [u8], String> {
        let n = self.len()?;
        self.take(n)
    }

    fn string(&mut self) -> Result<String, String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }
}

fn hex(bytes: &[u8]) -> Value {
    let shown = &bytes[..bytes.len().min(64)];
    let mut s = String::with_capacity(2 + shown.len() * 2 + 3);
    s.push_str("0x");
    for b in shown {
        s.push_str(&format!("{b:02x}"));
    }
    if bytes.len() > shown.len() {
        s.push('…');
    }
    Value::String(s)
}

fn float(f: f64) -> Value {
    Number::from_f64(f).map(Value::Number).unwrap_or_else(|| Value::String(f.to_string()))
}

/// Days since 1970-01-01 to (year, month, day).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn date(days: i64) -> String {
    let (y, m, d) = civil(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn time(micros_of_day: i64) -> String {
    let s = micros_of_day.div_euclid(1_000_000);
    let frac = micros_of_day.rem_euclid(1_000_000);
    let base = format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
    if frac == 0 {
        base
    } else if frac % 1000 == 0 {
        format!("{base}.{:03}", frac / 1000)
    } else {
        format!("{base}.{frac:06}")
    }
}

fn timestamp(micros: i64) -> String {
    let day = micros.div_euclid(86_400_000_000);
    format!("{}T{}Z", date(day), time(micros.rem_euclid(86_400_000_000)))
}

/// Big-endian two's-complement bytes with a decimal scale.
fn decimal(bytes: &[u8], scale: u32) -> Value {
    if bytes.is_empty() || bytes.len() > 16 {
        return hex(bytes);
    }
    let mut v: i128 = if bytes[0] & 0x80 != 0 { -1 } else { 0 };
    for b in bytes {
        v = (v << 8) | i128::from(*b);
    }
    let digits = v.unsigned_abs().to_string();
    let scale = scale as usize;
    let digits = if digits.len() <= scale { format!("{}{digits}", "0".repeat(scale + 1 - digits.len())) } else { digits };
    let (int, frac) = digits.split_at(digits.len() - scale);
    let sign = if v < 0 { "-" } else { "" };
    Value::String(if scale == 0 { format!("{sign}{int}") } else { format!("{sign}{int}.{frac}") })
}

fn text_or_hex(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(s) if !s.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r')) => Value::String(s.to_string()),
        _ => hex(bytes),
    }
}

fn resolve<'s>(candidates: &[String], names: &'s Names) -> Result<&'s Schema, String> {
    candidates.iter().find_map(|n| names.0.get(n)).ok_or_else(|| format!("The Avro schema refers to an unknown type \"{}\".", candidates.last().map(String::as_str).unwrap_or("")))
}

fn decode(schema: &Schema, r: &mut Reader, names: &Names, depth: usize) -> Result<Value, String> {
    if depth > 200 {
        return Err("The Avro data is nested too deeply.".into());
    }
    Ok(match schema {
        Schema::Null => Value::Null,
        Schema::Boolean => Value::Bool(r.take(1)?[0] != 0),
        Schema::Int | Schema::Long => Value::Number(r.long()?.into()),
        Schema::Float => float(f64::from(f32::from_le_bytes(r.take(4)?.try_into().unwrap()))),
        Schema::Double => float(f64::from_le_bytes(r.take(8)?.try_into().unwrap())),
        Schema::Bytes => text_or_hex(r.bytes()?),
        Schema::String => Value::String(r.string()?),
        Schema::Fixed(n) => hex(r.take(*n)?),
        Schema::Record(fields) => {
            let mut m = Map::with_capacity(fields.len());
            for (name, ty) in fields {
                m.insert(name.clone(), decode(ty, r, names, depth + 1)?);
            }
            Value::Object(m)
        }
        Schema::Enum(symbols) => {
            let i = r.long()?;
            Value::String(usize::try_from(i).ok().and_then(|i| symbols.get(i)).cloned().unwrap_or_else(|| format!("<symbol {i}>")))
        }
        Schema::Array(items) => {
            let mut out = Vec::new();
            for_each_block_item(r, |r| {
                out.push(decode(items, r, names, depth + 1)?);
                Ok(())
            })?;
            Value::Array(out)
        }
        Schema::Map(values) => {
            let mut out = Map::new();
            for_each_block_item(r, |r| {
                let key = r.string()?;
                out.insert(key, decode(values, r, names, depth + 1)?);
                Ok(())
            })?;
            Value::Object(out)
        }
        Schema::Union(branches) => {
            let i = r.long()?;
            let branch = usize::try_from(i).ok().and_then(|i| branches.get(i)).ok_or("The Avro data selects a union branch that does not exist.")?;
            decode(branch, r, names, depth + 1)?
        }
        Schema::Named(candidates) => decode(resolve(candidates, names)?, r, names, depth + 1)?,
        Schema::Logical(base, logical) => match (logical, base.as_ref()) {
            (Logical::Date, _) => Value::String(date(r.long()?)),
            (Logical::TimeMillis, _) => Value::String(time(r.long()?.saturating_mul(1000))),
            (Logical::TimeMicros, _) => Value::String(time(r.long()?)),
            (Logical::TimestampMillis, _) => Value::String(timestamp(r.long()?.saturating_mul(1000))),
            (Logical::TimestampMicros, _) => Value::String(timestamp(r.long()?)),
            (Logical::Decimal(scale), Schema::Fixed(n)) => decimal(r.take(*n)?, *scale),
            (Logical::Decimal(scale), _) => decimal(r.bytes()?, *scale),
        },
    })
}

/// Arrays and maps are written as blocks: a count, the items, and a zero count at the end.
fn for_each_block_item(r: &mut Reader, mut item: impl FnMut(&mut Reader) -> Result<(), String>) -> Result<(), String> {
    loop {
        let mut count = r.long()?;
        if count == 0 {
            return Ok(());
        }
        if count < 0 {
            count = count.checked_neg().ok_or("The Avro data contains an invalid block.")?;
            r.long()?; // byte size of the block, not needed
        }
        for _ in 0..count {
            item(r)?;
        }
    }
}

fn decompress(codec: &str, data: &[u8]) -> Result<Vec<u8>, String> {
    match codec {
        "deflate" => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(data).take(1 << 30).read_to_end(&mut out).map_err(|e| format!("Could not decompress the Avro data: {e}"))?;
            Ok(out)
        }
        "snappy" => {
            // the last four bytes are a CRC of the uncompressed data
            let body = data.len().checked_sub(4).map(|n| &data[..n]).ok_or("The Avro file ends unexpectedly.")?;
            snap::raw::Decoder::new().decompress_vec(body).map_err(|e| format!("Could not decompress the Avro data: {e}"))
        }
        other => Err(format!("This Avro file is compressed with \"{other}\", which Inkdown cannot read (supported: null, deflate, snappy).")),
    }
}

/// Reads a container file, decoding at most `limit` records.
pub fn read(bytes: &[u8], limit: usize) -> Result<File, String> {
    if !is_container(bytes) {
        return Err("This is not an Avro data file.".into());
    }
    let mut r = Reader::new(&bytes[MAGIC.len()..]);
    let mut meta: HashMap<String, Vec<u8>> = HashMap::new();
    for_each_block_item(&mut r, |r| {
        let key = r.string()?;
        meta.insert(key, r.bytes()?.to_vec());
        Ok(())
    })?;
    let sync = r.take(16)?.to_vec();
    let schema: Value = serde_json::from_slice(meta.get("avro.schema").ok_or("The Avro file has no schema.")?).map_err(|e| format!("The Avro schema is not valid JSON: {e}"))?;
    let codec = meta.get("avro.codec").map(|c| String::from_utf8_lossy(c).into_owned()).filter(|c| !c.is_empty()).unwrap_or_else(|| "null".into());
    let mut names = Names::default();
    let root = parse_schema(&schema, "", &mut names, 0)?;

    let mut records = Vec::new();
    let mut total: u64 = 0;
    while !r.done() {
        let count = u64::try_from(r.long()?).map_err(|_| "The Avro file contains an invalid block.".to_string())?;
        let block = r.bytes()?;
        if r.take(16)? != sync.as_slice() {
            return Err("The Avro file is damaged (a block marker does not match).".into());
        }
        total += count;
        if records.len() >= limit {
            continue; // only count the remaining blocks
        }
        let plain;
        let data = if codec == "null" {
            block
        } else {
            plain = decompress(&codec, block)?;
            &plain
        };
        let mut br = Reader::new(data);
        for _ in 0..count {
            if records.len() >= limit {
                break;
            }
            records.push(decode(&root, &mut br, &names, 0)?);
        }
    }
    Ok(File { schema, codec, records, total })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn long(out: &mut Vec<u8>, v: i64) {
        let mut z = ((v << 1) ^ (v >> 63)) as u64;
        loop {
            let b = (z & 0x7f) as u8;
            z >>= 7;
            if z == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
    }

    fn string(out: &mut Vec<u8>, s: &str) {
        long(out, s.len() as i64);
        out.extend_from_slice(s.as_bytes());
    }

    const SCHEMA: &str = r#"{"type":"record","name":"Reading","namespace":"demo","fields":[
        {"name":"sensor","type":"string"},
        {"name":"value","type":"double"},
        {"name":"count","type":"long"},
        {"name":"note","type":["null","string"]},
        {"name":"kind","type":{"type":"enum","name":"Kind","symbols":["TEMP","WIND"]}},
        {"name":"tags","type":{"type":"array","items":"string"}},
        {"name":"extra","type":{"type":"map","values":"int"}},
        {"name":"day","type":{"type":"int","logicalType":"date"}},
        {"name":"at","type":{"type":"long","logicalType":"timestamp-millis"}},
        {"name":"price","type":{"type":"bytes","logicalType":"decimal","precision":9,"scale":2}},
        {"name":"next","type":["null","Reading"]}
    ]}"#;

    fn record(out: &mut Vec<u8>, sensor: &str, nested: bool) {
        string(out, sensor);
        out.extend_from_slice(&21.5f64.to_le_bytes());
        long(out, -7);
        long(out, 1); // union branch: string
        string(out, "ok");
        long(out, 1); // WIND
        long(out, 2);
        string(out, "a");
        string(out, "b");
        long(out, 0);
        long(out, -1); // a map block with a byte size
        long(out, 3);
        string(out, "k");
        long(out, 42);
        long(out, 0);
        long(out, 19_000); // 2022-01-08
        long(out, 1_700_000_000_123);
        long(out, 2);
        out.extend_from_slice(&[0x30, 0x39]); // 12345 -> 123.45
        if nested {
            long(out, 1);
            record(out, "inner", false);
        } else {
            long(out, 0);
        }
    }

    fn container(codec: &str) -> Vec<u8> {
        let mut body = Vec::new();
        record(&mut body, "north", true);
        record(&mut body, "south", false);
        let block = match codec {
            "deflate" => {
                let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(&body).unwrap();
                e.finish().unwrap()
            }
            "snappy" => {
                let mut b = snap::raw::Encoder::new().compress_vec(&body).unwrap();
                b.extend_from_slice(&[0, 0, 0, 0]);
                b
            }
            _ => body,
        };
        let sync = [7u8; 16];
        let mut out = MAGIC.to_vec();
        long(&mut out, 2);
        string(&mut out, "avro.schema");
        string(&mut out, SCHEMA);
        string(&mut out, "avro.codec");
        string(&mut out, codec);
        long(&mut out, 0);
        out.extend_from_slice(&sync);
        long(&mut out, 2);
        long(&mut out, block.len() as i64);
        out.extend_from_slice(&block);
        out.extend_from_slice(&sync);
        out
    }

    #[test]
    fn reads_every_codec() {
        for codec in ["null", "deflate", "snappy"] {
            let f = read(&container(codec), 100).unwrap_or_else(|e| panic!("{codec}: {e}"));
            assert_eq!((f.codec.as_str(), f.total, f.records.len()), (codec, 2, 2));
            let r = &f.records[0];
            assert_eq!(r["sensor"], "north");
            assert_eq!(r["value"], 21.5);
            assert_eq!(r["count"], -7);
            assert_eq!(r["note"], "ok");
            assert_eq!(r["kind"], "WIND");
            assert_eq!(r["tags"], serde_json::json!(["a", "b"]));
            assert_eq!(r["extra"]["k"], 42);
            assert_eq!(r["day"], "2022-01-08");
            assert_eq!(r["at"], "2023-11-14T22:13:20.123Z");
            assert_eq!(r["price"], "123.45");
            assert_eq!(r["next"]["sensor"], "inner");
            assert!(f.records[1]["next"].is_null());
            // field order follows the schema
            assert_eq!(r.as_object().unwrap().keys().next().unwrap(), "sensor");
        }
    }

    #[test]
    fn limits_and_errors() {
        let f = read(&container("null"), 1).unwrap();
        assert_eq!((f.records.len(), f.total), (1, 2));
        assert!(f.truncated());
        let text = f.to_json_text();
        assert!(text.contains("\"shown\": 1") && text.contains("\"codec\": \"null\"") && text.contains("\"namespace\": \"demo\""));
        assert!(read(b"not avro", 10).is_err());
        let mut cut = container("null");
        cut.truncate(cut.len() - 20);
        assert!(read(&cut, 10).is_err());
        let mut bad = container("null");
        let n = bad.len();
        bad[n - 1] ^= 0xff;
        assert!(read(&bad, 10).unwrap_err().contains("damaged"));
        assert_eq!(decimal(&[0xff], 2), "-0.01");
        assert_eq!(date(-1), "1969-12-31");
        assert_eq!(time(3_723_000_000), "01:02:03");
    }
}

#[cfg(test)]
mod interop {
    /// Files written by another Avro implementation (fastavro), when present.
    #[test]
    fn reads_files_from_fastavro() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tools/demo/weather-station/data");
        let bytes = std::fs::read(dir.join("readings.avro")).expect("sample file");
        let f = super::read(&bytes, 1000).unwrap();
        assert_eq!((f.codec.as_str(), f.total, f.records.len()), ("deflate", 12, 12));
        let r = &f.records[2];
        assert_eq!(r["sensor"], "roof");
        assert_eq!(r["kind"], "WIND");
        assert_eq!(r["value"], 6.0);
        assert_eq!(r["samples"], 32);
        assert_eq!(r["taken"], "2026-11-01T08:00:00Z");
        assert_eq!(r["day"], "2026-11-01");
        assert_eq!(r["cost"], "13.00");
        assert!(r["note"].is_null());
        assert_eq!(f.records[0]["note"], "calibrated");
        assert_eq!(r["tags"], serde_json::json!(["outdoor", "wind"]));
        assert_eq!(r["limits"]["high"], 45.0);
        for extra in ["snappy.avro", "plain.avro"] {
            // written to the temp folder by tools/demo/make-avro.py; skipped when absent
            if let Ok(bytes) = std::fs::read(std::env::temp_dir().join("inkdown-demo").join(extra)) {
                let g = super::read(&bytes, 1000).unwrap_or_else(|e| panic!("{extra}: {e}"));
                assert_eq!(g.records, f.records, "{extra}");
            }
        }
    }
}
