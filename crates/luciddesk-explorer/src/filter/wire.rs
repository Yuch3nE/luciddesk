//! Explicit little-endian framing; no process-local pointers cross IPC.
pub const MAX_BYTES: usize = 1024 * 1024;
pub const MAX_ITEMS: usize = 4096;
pub const SET: u32 = 1;
pub const CLEAR_SELECTION: u32 = 2;
pub const PAUSE: u32 = 3;
pub const RESUME: u32 = 4;
pub const DETACH: u32 = 5;
pub const MENU_PREPARE: u32 = 6;
pub const MENU_FINISH: u32 = 7;
pub const UPDATE_BEGIN: u32 = 8;
pub const UPDATE_END: u32 = 9;
pub const REPLACE_IDENTITY: u32 = 10;
pub const MENU_CANCEL: u32 = 11;
pub const DESKTOP_GESTURE_ON: u32 = 12;
pub const DESKTOP_GESTURE_OFF: u32 = 13;
pub const DESKTOP_HIDE: u32 = 14;
pub const DESKTOP_RESTORE: u32 = 15;

pub fn is_desktop_visibility(op: u32) -> bool {
    (DESKTOP_GESTURE_ON..=DESKTOP_RESTORE).contains(&op)
}

/// Menu and rename-transaction failures use MENU_ERROR, without restoring
/// desktop membership. These requests also allow the caller to pump UI messages.
pub fn is_menu_transaction(op: u32) -> bool {
    matches!(
        op,
        MENU_PREPARE | MENU_CANCEL | MENU_FINISH | UPDATE_BEGIN | UPDATE_END | REPLACE_IDENTITY
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuContext {
    pub owner: u64,
    pub x: i32,
    pub y: i32,
}

pub struct Request {
    pub op: u32,
    pub sequence: u32,
    pub names: Vec<String>,
    pub menu: Option<MenuContext>,
}
fn valid_header(op: u32, sequence: u32, count: usize) -> bool {
    (SET..=DESKTOP_RESTORE).contains(&op)
        && sequence != 0
        && count <= MAX_ITEMS
        && (matches!(op, SET | MENU_PREPARE | REPLACE_IDENTITY) || count == 0)
        && (op != MENU_PREPARE || count != 0)
        && (op != REPLACE_IDENTITY || count == 2)
}
pub fn encode(op: u32, sequence: u32, names: &[String]) -> Result<Vec<u8>, String> {
    encode_request(op, sequence, names, None)
}
pub fn encode_menu(sequence: u32, names: &[String], menu: MenuContext) -> Result<Vec<u8>, String> {
    encode_request(MENU_PREPARE, sequence, names, Some(menu))
}
fn encode_request(
    op: u32,
    sequence: u32,
    names: &[String],
    menu: Option<MenuContext>,
) -> Result<Vec<u8>, String> {
    if !valid_header(op, sequence, names.len())
        || (op == MENU_PREPARE) != menu.is_some()
        || menu.is_some_and(|context| context.owner == 0)
    {
        return Err("桌面过滤请求无效".into());
    }
    let mut bytes = Vec::new();
    for value in [op, sequence, names.len() as u32] {
        bytes.extend(value.to_le_bytes());
    }
    if let Some(menu) = menu {
        bytes.extend(menu.owner.to_le_bytes());
        bytes.extend(menu.x.to_le_bytes());
        bytes.extend(menu.y.to_le_bytes());
    }
    for name in names {
        if name.is_empty() || name.contains('\0') {
            return Err("桌面项目标识无效".into());
        }
        let data: Vec<u16> = name.encode_utf16().collect();
        if data.len() > 32767 {
            return Err("桌面项目标识过长".into());
        }
        bytes.extend((data.len() as u32).to_le_bytes());
        for word in data {
            bytes.extend(word.to_le_bytes());
        }
        if bytes.len() > MAX_BYTES {
            return Err("桌面过滤请求过大".into());
        }
    }
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Option<Request> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let mut at = 0;
    let mut word = || {
        let value = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?);
        at += 4;
        Some(value)
    };
    let op = word()?;
    let sequence = word()?;
    let count = word()? as usize;
    if !valid_header(op, sequence, count) {
        return None;
    }
    let menu = if op == MENU_PREPARE {
        let owner = u64::from(word()?) | (u64::from(word()?) << 32);
        let x = word()? as i32;
        let y = word()? as i32;
        if owner == 0 {
            return None;
        }
        Some(MenuContext { owner, x, y })
    } else {
        None
    };
    let mut names = Vec::with_capacity(count);
    for _ in 0..count {
        let length = u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?) as usize;
        at += 4;
        if length == 0 || length > 32767 {
            return None;
        }
        let data = bytes.get(at..at + length * 2)?;
        at += length * 2;
        let utf16: Vec<_> = data
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let name = String::from_utf16(&utf16).ok()?;
        if name.contains('\0') {
            return None;
        }
        names.push(name);
    }
    (at == bytes.len()).then_some(Request {
        op,
        sequence,
        names,
        menu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_visibility_requests_are_bounded_and_separate_from_membership() {
        for op in DESKTOP_GESTURE_ON..=DESKTOP_RESTORE {
            let request = decode(&encode(op, 9, &[]).unwrap()).unwrap();
            assert_eq!(request.op, op);
            assert!(is_desktop_visibility(op));
            assert!(!is_menu_transaction(op));
            assert!(encode(op, 9, &["not allowed".into()]).is_err());
        }
        assert!(decode(&[16u32.to_le_bytes(), 1u32.to_le_bytes(), 0u32.to_le_bytes()].concat()).is_none());
    }
    #[test]
    fn rejects_truncated_oversized_and_invalid_requests() {
        let names = vec![
            "C:\\Users\\用户\\Desktop\\测试.lnk".into(),
            "::{645FF040-5081-101B-9F08-00AA002F954E}".into(),
        ];
        let bytes = encode(SET, 7, &names).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.names, names);
        assert_eq!(decoded.sequence, 7);
        for length in 0..bytes.len() {
            assert!(decode(&bytes[..length]).is_none());
        }
        let mut invalid = bytes.clone();
        invalid.push(0);
        assert!(decode(&invalid).is_none());
        invalid = bytes.clone();
        invalid[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&invalid).is_none());
        assert!(encode(SET, 0, &names).is_err());
        assert!(encode(PAUSE, 1, &names).is_err());
        assert!(encode(SET, 1, &["bad\0name".into()]).is_err());
        // Length is measured in UTF-16 units, including surrogate pairs.
        assert!(encode(SET, 1, &["a".repeat(32767)]).is_ok());
        assert!(encode(SET, 1, &["😀".repeat(16384)]).is_err());
        assert!(encode(SET, 1, &vec!["x".into(); MAX_ITEMS + 1]).is_err());
        assert!(encode(SET, 1, &vec!["x".repeat(32767); 17]).is_err());
        assert!(encode(UPDATE_BEGIN, 1, &names).is_err());
        assert!(encode(UPDATE_END, 1, &[]).is_ok());
        assert!(encode(REPLACE_IDENTITY, 1, &names[..1]).is_err());
        let renamed = decode(&encode(REPLACE_IDENTITY, 8, &names).unwrap()).unwrap();
        assert_eq!(renamed.names, names);
    }
    #[test]
    fn menu_frame_roundtrip_and_bounds() {
        let context = MenuContext {
            owner: 0x123456789,
            x: -1920,
            y: 240,
        };
        let names = vec!["C:\\Desktop\\项目.lnk".into()];
        let bytes = encode_menu(9, &names, context).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.menu, Some(context));
        assert_eq!(decoded.names, names);
        for len in 0..bytes.len() {
            assert!(decode(&bytes[..len]).is_none());
        }
        assert!(encode_menu(9, &[], context).is_err());
        assert!(
            encode_menu(
                9,
                &names,
                MenuContext {
                    owner: 0,
                    ..context
                }
            )
            .is_err()
        );
        assert!(encode(MENU_PREPARE, 9, &names).is_err());
    }
}
