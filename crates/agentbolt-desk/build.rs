//! Embed the shared bolt in Windows MSVC executables, without checking in binary assets.
//! Resource layout: https://learn.microsoft.com/windows/win32/menurc/resourceheader
//! Group layout: https://learn.microsoft.com/windows/win32/menurc/resdir
use std::{env, fs, path::PathBuf};

#[allow(dead_code)]
#[path = "../agentbolt-core/src/icon.rs"]
mod icon;

fn resource(out: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    for word in [0xffff, kind, 0xffff, id] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0x1030u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // Language-neutral artwork.
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(data);
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

fn bitmap(size: usize) -> Vec<u8> {
    let pixels = icon::rgba_at(size);
    let mask_stride = size.div_ceil(32) * 4;
    let mut dib = Vec::new();
    for value in [40u32, size as u32, (size * 2) as u32] {
        dib.extend_from_slice(&value.to_le_bytes());
    }
    dib.extend_from_slice(&1u16.to_le_bytes());
    dib.extend_from_slice(&32u16.to_le_bytes());
    dib.extend_from_slice(&[0; 24]); // BI_RGB, no palette.
    for row in pixels.chunks_exact(size * 4).rev() {
        for px in row.chunks_exact(4) {
            dib.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    for y in (0..size).rev() {
        let mut mask = vec![0u8; mask_stride];
        for x in 0..size {
            if pixels[(y * size + x) * 4 + 3] == 0 {
                mask[x / 8] |= 0x80 >> (x % 8);
            }
        }
        dib.extend_from_slice(&mask);
    }
    dib
}

fn main() {
    println!("cargo:rerun-if-changed=../agentbolt-core/src/icon.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let sizes = [16usize, 24, 32, 48, 64, 128, 256];
    let mut res = Vec::new();
    resource(&mut res, 0, 0, &[]); // Required null header.
    let mut group = vec![0, 0, 1, 0];
    group.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    for (index, size) in sizes.into_iter().enumerate() {
        let id = (index + 1) as u16;
        let dib = bitmap(size);
        resource(&mut res, 3, id, &dib); // RT_ICON
        group.extend_from_slice(&[size as u8, size as u8, 0, 0]);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        group.extend_from_slice(&id.to_le_bytes());
    }
    resource(&mut res, 14, 1, &group); // RT_GROUP_ICON
    let path = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("agentbolt.res");
    fs::write(&path, res).expect("write Agentbolt icon resource");
    println!("cargo:rustc-link-arg-bin=agentbolt-desk={}", path.display());
}
