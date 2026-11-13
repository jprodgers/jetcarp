//! The Windows resource file the build script writes (`version_res.rs`), read back entry by entry:
//! the empty first entry, the version resource, and string 1 of the string table, the name
//! Windows lists the screen saver by.

#[allow(dead_code)]
#[path = "../version_res.rs"]
mod version_res;

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// (type, name, data) of each entry.
fn entries(res: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    let mut out = vec![];
    let mut at = 0;
    while at < res.len() {
        let size = u32_at(res, at) as usize;
        let header = u32_at(res, at + 4) as usize;
        assert_eq!(header, 32, "numbered type and name");
        assert_eq!(u16_at(res, at + 8), 0xFFFF);
        assert_eq!(u16_at(res, at + 12), 0xFFFF);
        let (kind, name) = (u16_at(res, at + 10), u16_at(res, at + 14));
        let data = res[at + header..at + header + size].to_vec();
        out.push((kind, name, data));
        at += header + size;
        at = (at + 3) & !3;
    }
    assert_eq!(at, res.len());
    out
}

#[test]
fn the_resource_file_names_the_product_in_string_1() {
    let info = version_res::VersionInfo {
        numbers: [0, 4, 1, 0],
        version: "0.4.1",
        prerelease: false,
        product: "JETCARP",
        description: "JETCARP, started without a console window",
        company: "Jimmie Rodgers",
        copyright: "Jimmie Rodgers",
        file_name: "jetcarpw.exe",
    };
    let e = entries(&version_res::res_file(&info));
    assert_eq!(e.len(), 3);
    assert_eq!(
        (e[0].0, e[0].1, e[0].2.len()),
        (0, 0, 0),
        "the 32-bit marker"
    );
    assert_eq!((e[1].0, e[1].1), (16, 1), "RT_VERSION 1");
    assert_eq!((e[2].0, e[2].1), (6, 1), "RT_STRING block 1");
    // Block 1 holds strings 0 to 15: string 0 empty, string 1 the name, the rest empty.
    let b = &e[2].2;
    assert_eq!(u16_at(b, 0), 0);
    assert_eq!(u16_at(b, 2), 7);
    let name: Vec<u16> = (0..7).map(|i| u16_at(b, 4 + 2 * i)).collect();
    assert_eq!(String::from_utf16(&name).unwrap(), "JETCARP");
    assert_eq!(b.len(), 2 * 16 + 2 * 7);
    assert!((18..b.len()).step_by(2).all(|i| u16_at(b, i) == 0));
}
