//! A Windows version resource (`VERSIONINFO`), written as a compiled resource file (`.res`) in
//! plain Rust, with no resource compiler: the MSVC linker (and LLVM's `lld-link`) take a `.res`
//! file as an input and turn it into the program's resource section themselves. Windows shows the
//! fields in a program's Properties, Details; Windows Error Reporting names the version from it.
//!
//! The format: a `.res` file is a list of resources, each a header then its data, aligned to four
//! bytes, after an empty first entry that marks it as a 32-bit resource file. The version
//! resource's data is a `VS_VERSIONINFO` tree: each node is its length, its value's length, its
//! type (0 binary, 1 text), its key in UTF-16, then its value and its children, each aligned to
//! four bytes. See Microsoft's documentation of `RESOURCEHEADER`, `VS_VERSIONINFO`,
//! `VS_FIXEDFILEINFO`, `StringFileInfo` and `VarFileInfo`. A string table (string 1, the product
//! name) follows, for the screen saver list, then the program's icon (#105), from a `.ico` file
//! (`RT_ICON` and `RT_GROUP_ICON`; see `GRPICONDIR`).

/// What the resource says.
pub struct VersionInfo<'a> {
    /// The version's numbers: major, minor, patch, build.
    pub numbers: [u16; 4],
    /// The version as people read it (`0.4.0-rc.1`).
    pub version: &'a str,
    /// A pre-release (sets `VS_FF_PRERELEASE`).
    pub prerelease: bool,
    pub product: &'a str,
    pub description: &'a str,
    pub company: &'a str,
    pub copyright: &'a str,
    /// The program's file name (`jetcarp.exe`).
    pub file_name: &'a str,
    /// The program's icon: a `.ico` file, whose images become its icon resources (#105).
    pub icon: Option<&'a [u8]>,
}

/// US English, Unicode: the string table's language and code page.
const LANGUAGE: u16 = 0x0409;
const CODE_PAGE: u16 = 0x04B0;

fn utf16z(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn put16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn put32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

fn align4(b: &mut Vec<u8>) {
    while !b.len().is_multiple_of(4) {
        b.push(0);
    }
}

/// A node's value.
enum Value<'a> {
    None,
    Bytes(&'a [u8]),
    Text(&'a str),
}

/// One node of a `VS_VERSIONINFO` tree: `wLength`, `wValueLength` (bytes for binary, characters
/// with the terminator for text), `wType`, the key, then the value and the children, each
/// starting on a four-byte boundary. `wLength` covers everything up to the end of the last child.
fn node(key: &str, value: Value, children: &[Vec<u8>]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    for c in utf16z(key) {
        put16(&mut b, c);
    }
    align4(&mut b);
    let (value_len, kind) = match value {
        Value::None => (0, 1),
        Value::Bytes(v) => {
            b.extend_from_slice(v);
            (v.len(), 0)
        }
        Value::Text(s) => {
            let w = utf16z(s);
            for &c in &w {
                put16(&mut b, c);
            }
            (w.len(), 1)
        }
    };
    for c in children {
        align4(&mut b);
        b.extend_from_slice(c);
    }
    let len = b.len() as u16;
    b[0..2].copy_from_slice(&len.to_le_bytes());
    b[2..4].copy_from_slice(&(value_len as u16).to_le_bytes());
    b[4..6].copy_from_slice(&(kind as u16).to_le_bytes());
    b
}

/// `VS_FIXEDFILEINFO`: 52 bytes.
fn fixed_info(v: &VersionInfo) -> Vec<u8> {
    let [a, b, c, d] = v.numbers.map(u32::from);
    let mut f = Vec::with_capacity(52);
    put32(&mut f, 0xFEEF_04BD); // dwSignature
    put32(&mut f, 0x0001_0000); // dwStrucVersion
    put32(&mut f, (a << 16) | b); // dwFileVersionMS
    put32(&mut f, (c << 16) | d); // dwFileVersionLS
    put32(&mut f, (a << 16) | b); // dwProductVersionMS
    put32(&mut f, (c << 16) | d); // dwProductVersionLS
    put32(&mut f, 0x3F); // dwFileFlagsMask: VS_FFI_FILEFLAGSMASK
    put32(&mut f, if v.prerelease { 0x2 } else { 0 }); // dwFileFlags: VS_FF_PRERELEASE
    put32(&mut f, 0x0004_0004); // dwFileOS: VOS_NT_WINDOWS32
    put32(&mut f, 0x1); // dwFileType: VFT_APP
    put32(&mut f, 0); // dwFileSubtype
    put32(&mut f, 0); // dwFileDateMS
    put32(&mut f, 0); // dwFileDateLS
    f
}

/// The `VS_VERSIONINFO` tree.
pub fn version_info(v: &VersionInfo) -> Vec<u8> {
    let strings: Vec<Vec<u8>> = [
        ("CompanyName", v.company),
        ("FileDescription", v.description),
        ("FileVersion", v.version),
        ("InternalName", v.file_name.trim_end_matches(".exe")),
        ("LegalCopyright", v.copyright),
        ("OriginalFilename", v.file_name),
        ("ProductName", v.product),
        ("ProductVersion", v.version),
    ]
    .into_iter()
    .map(|(k, s)| node(k, Value::Text(s), &[]))
    .collect();
    let table = node(
        &format!("{LANGUAGE:04X}{CODE_PAGE:04X}"),
        Value::None,
        &strings,
    );
    let string_info = node("StringFileInfo", Value::None, &[table]);
    let mut translation = vec![];
    put16(&mut translation, LANGUAGE);
    put16(&mut translation, CODE_PAGE);
    let var = node("Translation", Value::Bytes(&translation), &[]);
    let var_info = node("VarFileInfo", Value::None, &[var]);
    let fixed = fixed_info(v);
    let mut root = node(
        "VS_VERSION_INFO",
        Value::Bytes(&fixed),
        &[string_info, var_info],
    );
    // The root is binary (its value is VS_FIXEDFILEINFO).
    root[4..6].copy_from_slice(&0u16.to_le_bytes());
    root
}

/// One resource entry of a `.res` file: a `RESOURCEHEADER` with a numbered type and name, then
/// the data, padded to four bytes.
fn entry(out: &mut Vec<u8>, kind: u16, name: u16, flags: u16, language: u16, data: &[u8]) {
    put32(out, data.len() as u32); // DataSize
    put32(out, 32); // HeaderSize
    put16(out, 0xFFFF); // TYPE: a number
    put16(out, kind);
    put16(out, 0xFFFF); // NAME: a number
    put16(out, name);
    put32(out, 0); // DataVersion
    put16(out, flags); // MemoryFlags
    put16(out, language); // LanguageId
    put32(out, 0); // Version
    put32(out, 0); // Characteristics
    out.extend_from_slice(data);
    align4(out);
}

/// The string table block that holds string 1: block 1 holds strings 0 to 15, each its length in
/// UTF-16 units and then its text, with no terminator; the others are empty.
pub fn string_block_1(text: &str) -> Vec<u8> {
    let mut b = vec![];
    for i in 0..16 {
        if i == 1 {
            let u: Vec<u16> = text.encode_utf16().collect();
            put16(&mut b, u.len() as u16);
            for c in u {
                put16(&mut b, c);
            }
        } else {
            put16(&mut b, 0);
        }
    }
    b
}

/// A `.res` file holding the version resource (`RT_VERSION`, number 1) and string 1 of the
/// string table (`RT_STRING`, block 1), the product's name: Windows lists a screen saver by its
/// string 1, and `JETCARP.scr` is a copy of `jetcarpw.exe` (without it, the list says
/// "Jetcarp", from the file's name). With an icon, its resources follow ([`icon_entries`]).
pub fn res_file(v: &VersionInfo) -> Vec<u8> {
    const RT_STRING: u16 = 6;
    const RT_VERSION: u16 = 16;
    // MOVEABLE | PURE, as the resource compiler marks a version resource; a string table is
    // DISCARDABLE as well.
    const FLAGS: u16 = 0x0030;
    const STRING_FLAGS: u16 = 0x1030;
    let mut out = vec![];
    // The empty first entry that marks a 32-bit resource file.
    entry(&mut out, 0, 0, 0, 0, &[]);
    entry(&mut out, RT_VERSION, 1, FLAGS, LANGUAGE, &version_info(v));
    entry(
        &mut out,
        RT_STRING,
        1,
        STRING_FLAGS,
        LANGUAGE,
        &string_block_1(v.product),
    );
    if let Some(ico) = v.icon {
        icon_entries(&mut out, ico);
    }
    out
}

/// An image of a `.ico` file: the first 12 bytes of its `ICONDIRENTRY` (width, height, colours,
/// reserved, planes, bits per pixel, size) and the image itself.
pub type IcoImage<'a> = ([u8; 12], &'a [u8]);

/// The images of a `.ico` file. The file is `ICONDIR` (reserved, type 1, count), one 16-byte entry
/// per image (the 12 bytes of [`IcoImage`] and the image's offset), then the images.
pub fn ico_images(ico: &[u8]) -> Result<Vec<IcoImage<'_>>, String> {
    let u16_at = |at: usize| {
        ico.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    if u16_at(0) != Some(0) || u16_at(2) != Some(1) {
        return Err("not a .ico file".into());
    }
    let count = u16_at(4).unwrap_or(0) as usize;
    (0..count)
        .map(|i| {
            let dir = ico
                .get(6 + 16 * i..6 + 16 * (i + 1))
                .ok_or("the .ico file's directory is cut short")?;
            let size = u32::from_le_bytes([dir[8], dir[9], dir[10], dir[11]]) as usize;
            let offset = u32::from_le_bytes([dir[12], dir[13], dir[14], dir[15]]) as usize;
            let image = ico
                .get(offset..offset + size)
                .ok_or("an image of the .ico file is cut short")?;
            let mut head = [0u8; 12];
            head.copy_from_slice(&dir[..12]);
            Ok((head, image))
        })
        .collect()
}

/// The program's icon, as the resource compiler lays out `1 ICON file.ico`: each image an icon
/// resource (`RT_ICON`, numbered from 1) holding it as it is in the file, and icon group 1
/// (`RT_GROUP_ICON`) listing them: `GRPICONDIR` (reserved, type 1, count), then a 14-byte
/// `GRPICONDIRENTRY` per image, its `ICONDIRENTRY`'s first 12 bytes and its resource's number.
/// Explorer, the taskbar and shortcuts show the program's first icon group.
fn icon_entries(out: &mut Vec<u8>, ico: &[u8]) {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    // MOVEABLE | DISCARDABLE, and the group PURE as well, as the resource compiler marks them.
    const ICON_FLAGS: u16 = 0x1010;
    const GROUP_FLAGS: u16 = 0x1030;
    let images = ico_images(ico).unwrap_or_else(|e| panic!("the icon: {e}"));
    let mut group = vec![];
    put16(&mut group, 0);
    put16(&mut group, 1);
    put16(&mut group, images.len() as u16);
    for (i, (head, image)) in images.iter().enumerate() {
        let id = i as u16 + 1;
        entry(out, RT_ICON, id, ICON_FLAGS, LANGUAGE, image);
        group.extend_from_slice(head);
        put16(&mut group, id);
    }
    entry(out, RT_GROUP_ICON, 1, GROUP_FLAGS, LANGUAGE, &group);
}
