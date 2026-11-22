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
        icon: None,
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

/// With the icon (#105): each image of the `.ico` file is an icon resource (`RT_ICON`, numbered
/// from 1) holding the image as it is, and icon group 1 (`RT_GROUP_ICON`) lists them, as the
/// resource compiler lays out `1 ICON jetcarp.ico`. Explorer, the taskbar and shortcuts show the
/// first group.
#[test]
fn the_icon_is_an_icon_group_of_the_ico_files_images() {
    let ico: &[u8] = include_bytes!("../jetcarp.ico");
    let info = version_res::VersionInfo {
        numbers: [0, 4, 3, 0],
        version: "0.4.3",
        prerelease: false,
        product: "JETCARP",
        description: "JETCARP",
        company: "Jimmie Rodgers",
        copyright: "Jimmie Rodgers",
        file_name: "jetcarp.exe",
        icon: Some(ico),
    };
    let e = entries(&version_res::res_file(&info));
    // The .ico file's own directory: an entry of 16 bytes for each image, after 6.
    let count = u16_at(ico, 4) as usize;
    assert_eq!(count, 7, "16, 24, 32, 48, 64, 128 and 256 px");
    let kinds: Vec<(u16, u16)> = e.iter().map(|x| (x.0, x.1)).collect();
    assert_eq!(e.len(), 3 + count + 1, "{kinds:?}");
    let group = e.iter().find(|x| x.0 == 14).expect("an RT_GROUP_ICON");
    assert_eq!(group.1, 1, "icon group 1");
    let g = &group.2;
    assert_eq!((u16_at(g, 0), u16_at(g, 2)), (0, 1));
    assert_eq!(u16_at(g, 4) as usize, count);
    assert_eq!(g.len(), 6 + 14 * count);
    for i in 0..count {
        let dir = &ico[6 + 16 * i..6 + 16 * (i + 1)];
        let (size, offset) = (u32_at(dir, 8) as usize, u32_at(dir, 12) as usize);
        let image = &ico[offset..offset + size];
        let entry = &g[6 + 14 * i..6 + 14 * (i + 1)];
        // Width, height, colours, reserved, planes, bits and size, as the file has them; then the
        // number of the icon resource that holds the image.
        assert_eq!(&entry[..12], &dir[..12], "image {i}");
        let id = u16_at(entry, 12);
        assert_eq!(id as usize, i + 1);
        let icon = e
            .iter()
            .find(|x| x.0 == 3 && x.1 == id)
            .expect("an RT_ICON");
        assert_eq!(icon.2, image, "image {i}, as it is in the file");
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
    }
    // The smallest first; 256 px is written as 0.
    assert_eq!(g[6], 16);
    assert_eq!(g[6 + 14 * (count - 1)], 0);
}
