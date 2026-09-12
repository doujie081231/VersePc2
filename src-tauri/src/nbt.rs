// nbt.rs — 最小化 Minecraft NBT 读写（大端序），用于 level.dat 解析与修改
// 支持全部 13 种 TAG 类型，读写均保留完整结构，避免修改时丢字段。
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub enum NbtValue {
    End,
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(Vec<NbtValue>),
    Compound(BTreeMap<String, NbtValue>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

// ============== 读取 ==============

/// 解析 NBT 二进制，返回 (根标签名, 根值)
pub fn read_nbt(bytes: &[u8]) -> Option<(String, NbtValue)> {
    let mut c = Cursor::new(bytes);
    let tag_type = read_u8(&mut c)?;
    if tag_type != 10 {
        return None;
    }
    let name = read_string(&mut c)?;
    let value = read_payload(tag_type, &mut c)?;
    Some((name, value))
}

fn read_payload(tag_type: u8, c: &mut Cursor<&[u8]>) -> Option<NbtValue> {
    match tag_type {
        0 => Some(NbtValue::End),
        1 => Some(NbtValue::Byte(read_i8(c)?)),
        2 => Some(NbtValue::Short(read_i16(c)?)),
        3 => Some(NbtValue::Int(read_i32(c)?)),
        4 => Some(NbtValue::Long(read_i64(c)?)),
        5 => Some(NbtValue::Float(read_f32(c)?)),
        6 => Some(NbtValue::Double(read_f64(c)?)),
        7 => {
            let len = read_i32(c)?;
            if len < 0 || len > 1_000_000 {
                return None;
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                v.push(read_i8(c)?);
            }
            Some(NbtValue::ByteArray(v))
        }
        8 => Some(NbtValue::String(read_string(c)?)),
        9 => {
            let elem = read_u8(c)?;
            let len = read_i32(c)?;
            if len < 0 || len > 10_000_000 {
                return None;
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                v.push(read_payload(elem, c)?);
            }
            Some(NbtValue::List(v))
        }
        10 => {
            let mut map = BTreeMap::new();
            loop {
                let t = read_u8(c)?;
                if t == 0 {
                    break;
                }
                let name = read_string(c)?;
                let val = read_payload(t, c)?;
                map.insert(name, val);
            }
            Some(NbtValue::Compound(map))
        }
        11 => {
            let len = read_i32(c)?;
            if len < 0 || len > 1_000_000 {
                return None;
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                v.push(read_i32(c)?);
            }
            Some(NbtValue::IntArray(v))
        }
        12 => {
            let len = read_i32(c)?;
            if len < 0 || len > 1_000_000 {
                return None;
            }
            let mut v = Vec::with_capacity(len as usize);
            for _ in 0..len {
                v.push(read_i64(c)?);
            }
            Some(NbtValue::LongArray(v))
        }
        _ => None,
    }
}

// ============== 写入 ==============

/// 序列化 NBT（根必须是 Compound）
pub fn write_nbt(name: &str, root: &NbtValue) -> Vec<u8> {
    let mut out = Vec::new();
    if let NbtValue::Compound(_) = root {
        out.push(10);
        write_string(&mut out, name);
        write_payload(&mut out, root);
    }
    out
}

fn write_payload(out: &mut Vec<u8>, v: &NbtValue) {
    match v {
        NbtValue::End => out.push(0),
        NbtValue::Byte(x) => write_i8(out, *x),
        NbtValue::Short(x) => write_i16(out, *x),
        NbtValue::Int(x) => write_i32(out, *x),
        NbtValue::Long(x) => write_i64(out, *x),
        NbtValue::Float(x) => write_f32(out, *x),
        NbtValue::Double(x) => write_f64(out, *x),
        NbtValue::ByteArray(arr) => {
            write_i32(out, arr.len() as i32);
            for x in arr {
                write_i8(out, *x);
            }
        }
        NbtValue::String(s) => write_string(out, s),
        NbtValue::List(list) => {
            let elem = list.first().map(tag_type_of).unwrap_or(0);
            out.push(elem);
            write_i32(out, list.len() as i32);
            for x in list {
                write_payload(out, x);
            }
        }
        NbtValue::Compound(map) => {
            for (name, val) in map {
                if matches!(val, NbtValue::End) {
                    continue;
                }
                out.push(tag_type_of(val));
                write_string(out, name);
                write_payload(out, val);
            }
            out.push(0);
        }
        NbtValue::IntArray(arr) => {
            write_i32(out, arr.len() as i32);
            for x in arr {
                write_i32(out, *x);
            }
        }
        NbtValue::LongArray(arr) => {
            write_i32(out, arr.len() as i32);
            for x in arr {
                write_i64(out, *x);
            }
        }
    }
}

fn tag_type_of(v: &NbtValue) -> u8 {
    match v {
        NbtValue::End => 0,
        NbtValue::Byte(_) => 1,
        NbtValue::Short(_) => 2,
        NbtValue::Int(_) => 3,
        NbtValue::Long(_) => 4,
        NbtValue::Float(_) => 5,
        NbtValue::Double(_) => 6,
        NbtValue::ByteArray(_) => 7,
        NbtValue::String(_) => 8,
        NbtValue::List(_) => 9,
        NbtValue::Compound(_) => 10,
        NbtValue::IntArray(_) => 11,
        NbtValue::LongArray(_) => 12,
    }
}

// ============== level.dat（gzip + NBT） ==============

/// 读取 level.dat（gzip 解压 + NBT 解析）
pub fn read_level_dat(path: &Path) -> Option<(String, NbtValue)> {
    let raw = std::fs::read(path).ok()?;
    let mut dec = flate2::read::GzDecoder::new(&raw[..]);
    let mut bytes = Vec::new();
    dec.read_to_end(&mut bytes).ok()?;
    read_nbt(&bytes)
}

/// 写回 level.dat（NBT 序列化 + gzip 压缩）
pub fn write_level_dat(path: &Path, name: &str, root: &NbtValue) -> Result<(), String> {
    let bytes = write_nbt(name, root);
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&bytes).map_err(|e| e.to_string())?;
    let gz = enc.finish().map_err(|e| e.to_string())?;
    std::fs::write(path, gz).map_err(|e| e.to_string())
}

// ============== 便捷访问（Compound 辅助） ==============

pub fn as_compound(v: &NbtValue) -> Option<&BTreeMap<String, NbtValue>> {
    match v {
        NbtValue::Compound(m) => Some(m),
        _ => None,
    }
}

pub fn as_compound_mut(v: &mut NbtValue) -> Option<&mut BTreeMap<String, NbtValue>> {
    match v {
        NbtValue::Compound(m) => Some(m),
        _ => None,
    }
}

pub fn get_compound<'a>(v: &'a NbtValue, key: &str) -> Option<&'a BTreeMap<String, NbtValue>> {
    let map = as_compound(v)?;
    as_compound(map.get(key)?)
}

pub fn get_compound_mut<'a>(
    v: &'a mut NbtValue,
    key: &str,
) -> Option<&'a mut BTreeMap<String, NbtValue>> {
    let map = as_compound_mut(v)?;
    as_compound_mut(map.get_mut(key)?)
}

pub fn compound_string(map: &BTreeMap<String, NbtValue>, key: &str) -> Option<String> {
    match map.get(key) {
        Some(NbtValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

pub fn compound_byte(map: &BTreeMap<String, NbtValue>, key: &str) -> Option<i8> {
    match map.get(key) {
        Some(NbtValue::Byte(b)) => Some(*b),
        _ => None,
    }
}

pub fn compound_long(map: &BTreeMap<String, NbtValue>, key: &str) -> Option<i64> {
    match map.get(key) {
        Some(NbtValue::Long(l)) => Some(*l),
        _ => None,
    }
}

pub fn as_string(v: &NbtValue) -> Option<&str> {
    match v {
        NbtValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

// ============== 基础读写原语 ==============

fn read_u8(c: &mut Cursor<&[u8]>) -> Option<u8> {
    let mut b = [0u8; 1];
    c.read_exact(&mut b).ok()?;
    Some(b[0])
}
fn read_i8(c: &mut Cursor<&[u8]>) -> Option<i8> {
    Some(read_u8(c)? as i8)
}
fn read_i16(c: &mut Cursor<&[u8]>) -> Option<i16> {
    let mut b = [0u8; 2];
    c.read_exact(&mut b).ok()?;
    Some(i16::from_be_bytes(b))
}
fn read_i32(c: &mut Cursor<&[u8]>) -> Option<i32> {
    let mut b = [0u8; 4];
    c.read_exact(&mut b).ok()?;
    Some(i32::from_be_bytes(b))
}
fn read_i64(c: &mut Cursor<&[u8]>) -> Option<i64> {
    let mut b = [0u8; 8];
    c.read_exact(&mut b).ok()?;
    Some(i64::from_be_bytes(b))
}
fn read_f32(c: &mut Cursor<&[u8]>) -> Option<f32> {
    let mut b = [0u8; 4];
    c.read_exact(&mut b).ok()?;
    Some(f32::from_be_bytes(b))
}
fn read_f64(c: &mut Cursor<&[u8]>) -> Option<f64> {
    let mut b = [0u8; 8];
    c.read_exact(&mut b).ok()?;
    Some(f64::from_be_bytes(b))
}
fn read_string(c: &mut Cursor<&[u8]>) -> Option<String> {
    let len = read_i16(c)?;
    if len < 0 {
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    c.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

fn write_i8(out: &mut Vec<u8>, x: i8) {
    out.push(x as u8);
}
fn write_i16(out: &mut Vec<u8>, x: i16) {
    out.extend_from_slice(&x.to_be_bytes());
}
fn write_i32(out: &mut Vec<u8>, x: i32) {
    out.extend_from_slice(&x.to_be_bytes());
}
fn write_i64(out: &mut Vec<u8>, x: i64) {
    out.extend_from_slice(&x.to_be_bytes());
}
fn write_f32(out: &mut Vec<u8>, x: f32) {
    out.extend_from_slice(&x.to_be_bytes());
}
fn write_f64(out: &mut Vec<u8>, x: f64) {
    out.extend_from_slice(&x.to_be_bytes());
}
fn write_string(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    write_i16(out, bytes.len() as i16);
    out.extend_from_slice(bytes);
}
