//! What every `server.properties` key means, and what kind of control edits it.
//!
//! The old shape was a struct with nine fields. That models the nine settings
//! this app happened to expose, not the file — so the other forty-odd keys had
//! no UI at all and "edit the raw file" became the only way to change the port.
//! A table instead: each row says what a key is called in Chinese, which group
//! it belongs to, and what type it holds. The UI renders a control per row.
//!
//! Two properties this file exists to keep:
//!
//! * **A key we do not know is still shown.** Mods add keys, and new Minecraft
//!   versions add more. Anything in the file without a row here is offered as
//!   a text field rather than hidden — and, because writes merge key by key,
//!   never dropped.
//! * **The file stays the source of truth.** Nothing here caches values. The
//!   defaults below are what Minecraft writes on a fresh install, used only
//!   when the file has no such line yet.

use serde::{Deserialize, Serialize};

use crate::types::RawProperties;

/// Which section of the settings page a key appears under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PropertyGroup {
    /// Who can reach the server and on what.
    Connection,
    /// Rules of play.
    Gameplay,
    /// The world itself.
    World,
    /// Everything else, including keys this app has no row for.
    Advanced,
}

/// The control the UI should draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PropertyKind {
    Bool,
    /// A fixed set of values. `options` carries the raw value and its label.
    Enum { options: Vec<EnumOption> },
    /// A number. The bounds are Minecraft's, where it has them.
    Int { min: Option<i64>, max: Option<i64> },
    /// Free text — and the fallback for a key with no row in the table.
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumOption {
    pub value: String,
    pub label: String,
}

/// One editable line of the file, as the UI sees it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyField {
    pub key: String,
    /// Chinese label, or the key itself when this app has no row for it.
    pub label: String,
    pub group: PropertyGroup,
    #[serde(flatten)]
    pub kind: PropertyKind,
    /// Current value, verbatim from the file — or the Minecraft default when
    /// the file has no line for it yet.
    pub value: String,
    /// A short note under the control. Empty for the obvious ones.
    pub hint: String,
    /// False when the key is not in this table: shown as text, at the bottom.
    pub known: bool,
}

struct Row {
    key: &'static str,
    label: &'static str,
    group: PropertyGroup,
    kind: Kind,
    default: &'static str,
    hint: &'static str,
}

/// The compile-time half of `PropertyKind` — enums as flat pairs so the table
/// below stays readable.
enum Kind {
    Bool,
    Enum(&'static [(&'static str, &'static str)]),
    Int(Option<i64>, Option<i64>),
    Text,
}

use PropertyGroup::{Advanced, Connection, Gameplay, World};

const GAMEMODES: &[(&str, &str)] = &[
    ("survival", "生存"),
    ("creative", "創造"),
    ("adventure", "冒險"),
    ("spectator", "旁觀"),
];

const DIFFICULTIES: &[(&str, &str)] = &[
    ("peaceful", "和平"),
    ("easy", "簡單"),
    ("normal", "普通"),
    ("hard", "困難"),
];

const LEVEL_TYPES: &[(&str, &str)] = &[
    ("minecraft:normal", "一般"),
    ("minecraft:flat", "超平坦"),
    ("minecraft:large_biomes", "巨型生態域"),
    ("minecraft:amplified", "放大化"),
    ("minecraft:single_biome_surface", "單一生態域"),
];

/// Order here is the order on screen, within each group.
const TABLE: &[Row] = &[
    // ── 連線 ────────────────────────────────────────────
    Row { key: "motd", label: "MOTD", group: Connection, kind: Kind::Text,
          default: "A Minecraft Server", hint: "玩家在伺服器清單看到的那行字" },
    Row { key: "server-port", label: "連接埠", group: Connection,
          kind: Kind::Int(Some(1), Some(65535)), default: "25565", hint: "" },
    Row { key: "server-ip", label: "綁定位址", group: Connection, kind: Kind::Text,
          default: "", hint: "留空表示接受所有網路介面" },
    Row { key: "max-players", label: "最大玩家", group: Connection,
          kind: Kind::Int(Some(0), None), default: "20", hint: "" },
    Row { key: "online-mode", label: "正版驗證", group: Connection, kind: Kind::Bool,
          default: "true", hint: "關閉後非正版帳號可加入" },
    Row { key: "white-list", label: "白名單", group: Connection, kind: Kind::Bool,
          default: "false", hint: "名單本身用主控台的 /whitelist add 維護" },
    Row { key: "enforce-whitelist", label: "強制白名單", group: Connection, kind: Kind::Bool,
          default: "false", hint: "開啟後不在名單上的玩家會被立刻踢出" },
    Row { key: "prevent-proxy-connections", label: "阻擋代理連線", group: Connection,
          kind: Kind::Bool, default: "false", hint: "" },
    Row { key: "player-idle-timeout", label: "閒置踢出（分鐘）", group: Connection,
          kind: Kind::Int(Some(0), None), default: "0", hint: "0 表示不踢" },
    Row { key: "network-compression-threshold", label: "網路壓縮門檻", group: Connection,
          kind: Kind::Int(Some(-1), None), default: "256", hint: "-1 關閉壓縮" },
    Row { key: "rate-limit", label: "封包速率上限", group: Connection,
          kind: Kind::Int(Some(0), None), default: "0", hint: "0 表示不限制" },

    // ── 玩法 ────────────────────────────────────────────
    Row { key: "gamemode", label: "遊戲模式", group: Gameplay, kind: Kind::Enum(GAMEMODES),
          default: "survival", hint: "" },
    Row { key: "force-gamemode", label: "強制遊戲模式", group: Gameplay, kind: Kind::Bool,
          default: "false", hint: "每次加入都拉回預設模式" },
    Row { key: "difficulty", label: "難度", group: Gameplay, kind: Kind::Enum(DIFFICULTIES),
          default: "easy", hint: "" },
    Row { key: "hardcore", label: "極限模式", group: Gameplay, kind: Kind::Bool,
          default: "false", hint: "死亡後轉為旁觀者" },
    Row { key: "pvp", label: "PVP", group: Gameplay, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "allow-flight", label: "允許飛行", group: Gameplay, kind: Kind::Bool,
          default: "false", hint: "關閉時飛行過久會被踢出；裝了飛行類模組要開" },
    Row { key: "spawn-monsters", label: "生成怪物", group: Gameplay, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "spawn-animals", label: "生成動物", group: Gameplay, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "spawn-npcs", label: "生成村民", group: Gameplay, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "enable-command-block", label: "指令方塊", group: Gameplay, kind: Kind::Bool,
          default: "false", hint: "" },
    Row { key: "op-permission-level", label: "OP 權限等級", group: Gameplay,
          kind: Kind::Int(Some(1), Some(4)), default: "4", hint: "" },

    // ── 世界 ────────────────────────────────────────────
    Row { key: "level-name", label: "世界資料夾", group: World, kind: Kind::Text,
          default: "world", hint: "改成不存在的名字等於開新世界" },
    Row { key: "level-seed", label: "世界種子", group: World, kind: Kind::Text,
          default: "", hint: "只在世界第一次生成時有用" },
    Row { key: "level-type", label: "世界類型", group: World, kind: Kind::Enum(LEVEL_TYPES),
          default: "minecraft:normal", hint: "" },
    Row { key: "generate-structures", label: "生成建築", group: World, kind: Kind::Bool,
          default: "true", hint: "村莊、要塞、遺跡" },
    Row { key: "allow-nether", label: "允許地獄", group: World, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "view-distance", label: "視野距離", group: World,
          kind: Kind::Int(Some(3), Some(32)), default: "10", hint: "越大越吃記憶體與頻寬" },
    Row { key: "simulation-distance", label: "模擬距離", group: World,
          kind: Kind::Int(Some(3), Some(32)), default: "10", hint: "紅石與生物運作的範圍" },
    Row { key: "spawn-protection", label: "重生點保護（格）", group: World,
          kind: Kind::Int(Some(0), None), default: "16", hint: "0 表示不保護" },
    Row { key: "max-world-size", label: "世界半徑上限", group: World,
          kind: Kind::Int(Some(1), Some(29999984)), default: "29999984", hint: "" },
    Row { key: "max-build-height", label: "建築高度上限", group: World,
          kind: Kind::Int(Some(0), None), default: "256", hint: "" },

    // ── 進階 ────────────────────────────────────────────
    Row { key: "enable-rcon", label: "啟用 RCON", group: Advanced, kind: Kind::Bool,
          default: "false", hint: "這個 app 用主控台直接下指令，不需要 RCON" },
    Row { key: "rcon.port", label: "RCON 連接埠", group: Advanced,
          kind: Kind::Int(Some(1), Some(65535)), default: "25575", hint: "" },
    Row { key: "rcon.password", label: "RCON 密碼", group: Advanced, kind: Kind::Text,
          default: "", hint: "" },
    Row { key: "enable-query", label: "啟用查詢協定", group: Advanced, kind: Kind::Bool,
          default: "false", hint: "" },
    Row { key: "query.port", label: "查詢連接埠", group: Advanced,
          kind: Kind::Int(Some(1), Some(65535)), default: "25565", hint: "" },
    Row { key: "enable-status", label: "回應狀態查詢", group: Advanced, kind: Kind::Bool,
          default: "true", hint: "關閉後伺服器清單顯示不出資訊" },
    Row { key: "hide-online-players", label: "隱藏線上玩家", group: Advanced, kind: Kind::Bool,
          default: "false", hint: "" },
    Row { key: "broadcast-console-to-ops", label: "指令結果廣播給 OP", group: Advanced,
          kind: Kind::Bool, default: "true", hint: "" },
    Row { key: "broadcast-rcon-to-ops", label: "RCON 結果廣播給 OP", group: Advanced,
          kind: Kind::Bool, default: "true", hint: "" },
    Row { key: "function-permission-level", label: "函式權限等級", group: Advanced,
          kind: Kind::Int(Some(1), Some(4)), default: "2", hint: "" },
    Row { key: "resource-pack", label: "資源包網址", group: Advanced, kind: Kind::Text,
          default: "", hint: "" },
    Row { key: "require-resource-pack", label: "強制資源包", group: Advanced, kind: Kind::Bool,
          default: "false", hint: "拒絕下載的玩家無法加入" },
    Row { key: "enforce-secure-profile", label: "強制安全聊天簽章", group: Advanced,
          kind: Kind::Bool, default: "true", hint: "關閉後未簽章的客戶端才能加入" },
    Row { key: "sync-chunk-writes", label: "同步寫入區塊", group: Advanced, kind: Kind::Bool,
          default: "true", hint: "關閉較快，但當機時世界更可能損毀" },
    Row { key: "use-native-transport", label: "使用原生傳輸", group: Advanced, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "entity-broadcast-range-percentage", label: "實體廣播範圍（%）", group: Advanced,
          kind: Kind::Int(Some(10), Some(1000)), default: "100", hint: "" },
    Row { key: "max-tick-time", label: "單次 tick 上限（毫秒）", group: Advanced,
          kind: Kind::Int(Some(-1), None), default: "60000", hint: "-1 關閉看門狗" },
    Row { key: "log-ips", label: "記錄 IP", group: Advanced, kind: Kind::Bool,
          default: "true", hint: "" },
    Row { key: "enable-jmx-monitoring", label: "JMX 監控", group: Advanced, kind: Kind::Bool,
          default: "false", hint: "" },
];

fn kind_of(kind: &Kind) -> PropertyKind {
    match kind {
        Kind::Bool => PropertyKind::Bool,
        Kind::Enum(options) => PropertyKind::Enum {
            options: options
                .iter()
                .map(|(value, label)| EnumOption {
                    value: (*value).to_owned(),
                    label: (*label).to_owned(),
                })
                .collect(),
        },
        Kind::Int(min, max) => PropertyKind::Int {
            min: *min,
            max: *max,
        },
        Kind::Text => PropertyKind::Text,
    }
}

/// Every field the settings page should draw, in display order.
///
/// Known keys first, in table order; then whatever else the file holds, as
/// text, alphabetically. A modded server has keys nobody has a label for, and
/// hiding them is how a setting silently becomes uneditable.
pub fn fields(raw: &RawProperties) -> Vec<PropertyField> {
    let mut out: Vec<PropertyField> = TABLE
        .iter()
        .map(|row| PropertyField {
            key: row.key.to_owned(),
            label: row.label.to_owned(),
            group: row.group,
            kind: kind_of(&row.kind),
            value: raw
                .get(row.key)
                .cloned()
                .unwrap_or_else(|| row.default.to_owned()),
            hint: row.hint.to_owned(),
            known: true,
        })
        .collect();

    let mut unknown: Vec<&String> = raw
        .keys()
        .filter(|k| !TABLE.iter().any(|row| row.key == k.as_str()))
        .collect();
    unknown.sort();

    out.extend(unknown.into_iter().map(|key| PropertyField {
        key: key.clone(),
        label: key.clone(),
        group: Advanced,
        kind: PropertyKind::Text,
        value: raw.get(key).cloned().unwrap_or_default(),
        hint: String::new(),
        known: false,
    }));
    out
}

/// Whether a key is one this app will write.
///
/// The UI sends back whatever it was given, but the values cross the IPC
/// boundary: a key with a newline in it would inject a second line into the
/// file, and one with `=` would rewrite a different setting than the one named.
pub fn is_writable_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Values are single-line by definition of the format.
pub fn is_writable_value(value: &str) -> bool {
    value.len() <= 4096 && !value.contains(['\n', '\r'])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_no_duplicate_keys() {
        let mut keys: Vec<&str> = TABLE.iter().map(|r| r.key).collect();
        let before = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), before, "a duplicated key would render twice");
    }

    #[test]
    fn an_enum_default_is_one_of_its_options() {
        for row in TABLE {
            if let Kind::Enum(options) = &row.kind {
                assert!(
                    options.iter().any(|(v, _)| *v == row.default),
                    "{}: default {:?} is not in its option list",
                    row.key,
                    row.default
                );
            }
        }
    }

    #[test]
    fn a_missing_line_falls_back_to_minecrafts_default() {
        let fields = fields(&RawProperties::new());
        let port = fields.iter().find(|f| f.key == "server-port").unwrap();
        assert_eq!(port.value, "25565");
        assert!(matches!(port.kind, PropertyKind::Int { .. }));
    }

    #[test]
    fn the_file_wins_over_the_default() {
        let mut raw = RawProperties::new();
        raw.insert("server-port".into(), "25570".into());
        let fields = fields(&raw);
        assert_eq!(
            fields.iter().find(|f| f.key == "server-port").unwrap().value,
            "25570"
        );
    }

    #[test]
    fn a_key_with_no_row_is_offered_as_text_rather_than_hidden() {
        let mut raw = RawProperties::new();
        raw.insert("some-mod-setting".into(), "42".into());
        let fields = fields(&raw);

        let extra = fields.iter().find(|f| f.key == "some-mod-setting").unwrap();
        assert!(!extra.known);
        assert_eq!(extra.kind, PropertyKind::Text);
        assert_eq!(extra.group, Advanced);
        // Unknown keys sort after every known one, so the form does not open
        // with a wall of things nobody has a label for.
        let first_unknown = fields.iter().position(|f| !f.known).unwrap();
        assert!(fields[..first_unknown].iter().all(|f| f.known));
    }

    #[test]
    fn a_crafted_key_or_value_cannot_reach_the_file() {
        assert!(is_writable_key("server-port"));
        assert!(is_writable_key("rcon.port"));
        for bad in ["", "a b", "a=b", "a\nb", "键"] {
            assert!(!is_writable_key(bad), "must refuse key {bad:?}");
        }
        assert!(is_writable_value("A Minecraft Server"));
        // A newline would append a second, unrelated setting.
        assert!(!is_writable_value("hi\nop-permission-level=4"));
    }
}
