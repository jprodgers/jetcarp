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
//! `VS_FIXEDFILEINFO`, `StringFileInfo` and `VarFileInfo`.

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
    while b.len() % 4 != 0 {
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

/// A `.res` file holding the version resource (`RT_VERSION`, number 1).
pub fn res_file(v: &VersionInfo) -> Vec<u8> {
    const RT_VERSION: u16 = 16;
    // MOVEABLE | PURE, as the resource compiler marks a version resource.
    const FLAGS: u16 = 0x0030;
    let mut out = vec![];
    // The empty first entry that marks a 32-bit resource file.
    entry(&mut out, 0, 0, 0, 0, &[]);
    entry(&mut out, RT_VERSION, 1, FLAGS, LANGUAGE, &version_info(v));
    out
}
