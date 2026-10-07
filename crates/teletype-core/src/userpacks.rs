//! User-created vocabulary packs (item 15): named collections of words the
//! user teaches Teletype about their own world — company names, tools,
//! projects, people — so dictation stops mangling them.
//!
//! A user pack is a `DictionaryWord`-shaped term plus a `pack_id` that groups
//! it. The correction pipeline consumes them through the same `PackTerm`
//! tier as the bundled packs: a user-pack word with no known mis-hearing
//! still anchors exact + fuzzy matching exactly like a dictionary word.

use serde::{Deserialize, Serialize};

/// One user-created pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// canonical -> known mis-hearings (user-taught aliases).
    #[serde(default)]
    pub terms: Vec<UserPackTerm>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: u64,
}

/// One word inside a user pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPackTerm {
    pub canonical: String,
    #[serde(default)]
    pub mishearings: Vec<String>,
}

/// The persisted collection of user packs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPackStore {
    pub packs: Vec<UserPack>,
}

impl UserPackStore {
    pub fn get(&self, id: &str) -> Option<&UserPack> {
        self.packs.iter().find(|p| p.id == id)
    }

    pub fn insert(&mut self, pack: UserPack) -> Result<(), String> {
        if self.get(&pack.id).is_some() {
            return Err("Pack id already exists".into());
        }
        self.packs.push(pack);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.packs.len();
        self.packs.retain(|p| p.id != id);
        self.packs.len() != before
    }

    /// A fresh pack.
    pub fn new_pack(name: &str, description: &str) -> UserPack {
        UserPack {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            description: description.to_string(),
            terms: Vec::new(),
            enabled: true,
            created_at: crate::storage::now_ms(),
        }
    }

    /// Adds a term to a pack. An existing term (case-insensitive) gets its
    /// mis-hearings merged instead of being duplicated.
    pub fn add_term(&mut self, pack_id: &str, canonical: &str, mishearings: &[String]) -> Result<(), String> {
        let pack = self
            .packs
            .iter_mut()
            .find(|p| p.id == pack_id)
            .ok_or_else(|| "Unknown user pack".to_string())?;
        let canonical = canonical.trim();
        if canonical.is_empty() {
            return Err("Word can't be empty".into());
        }
        if let Some(existing) = pack
            .terms
            .iter_mut()
            .find(|t| t.canonical.eq_ignore_ascii_case(canonical))
        {
            for m in mishearings {
                let m = m.trim();
                if !m.is_empty() && !existing.mishearings.iter().any(|x| x.eq_ignore_ascii_case(m)) {
                    existing.mishearings.push(m.to_string());
                }
            }
        } else {
            pack.terms.push(UserPackTerm {
                canonical: canonical.to_string(),
                mishearings: mishearings
                    .iter()
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty())
                    .collect(),
            });
        }
        Ok(())
    }

    /// Removes one word from a pack. Returns true when something changed.
    pub fn remove_term(&mut self, pack_id: &str, canonical: &str) -> bool {
        let Some(pack) = self.packs.iter_mut().find(|p| p.id == pack_id) else {
            return false;
        };
        let before = pack.terms.len();
        pack.terms.retain(|t| !t.canonical.eq_ignore_ascii_case(canonical));
        pack.terms.len() != before
    }

    /// Flattens the enabled user packs into correction-tier terms. A word with
    /// no mis-hearings still yields one self-pair so exact matching anchors it
    /// (the corrector's exact pass runs before the fuzzy tier).
    pub fn terms_for(&self) -> Vec<crate::vocab::PackTerm> {
        let mut out = Vec::new();
        for p in self.packs.iter().filter(|p| p.enabled) {
            for t in &p.terms {
                let c = t.canonical.to_lowercase();
                let aliases: Vec<String> = t.mishearings.iter().map(|m| m.to_lowercase()).collect();
                if aliases.is_empty() {
                    out.push(crate::vocab::PackTerm {
                        canonical: c.clone(),
                        alias: c,
                    });
                } else {
                    for a in aliases {
                        out.push(crate::vocab::PackTerm {
                            canonical: c.clone(),
                            alias: a,
                        });
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove_terms() {
        let mut store = UserPackStore::default();
        let pack = UserPackStore::new_pack("My company", "Names and tools");
        let id = pack.id.clone();
        store.insert(pack).unwrap();
        store.add_term(&id, "Teletype", &["teletype".into()]).unwrap();
        store.add_term(&id, "Teletype", &["teletipe".into()]).unwrap();
        let p = store.get(&id).unwrap();
        assert_eq!(p.terms.len(), 1);
        assert_eq!(p.terms[0].mishearings.len(), 2);
        assert!(store.remove_term(&id, "teletype"));
        assert!(store.get(&id).unwrap().terms.is_empty());
    }

    #[test]
    fn terms_for_only_enabled_packs() {
        let mut store = UserPackStore::default();
        let mut pack = UserPackStore::new_pack("P", "");
        pack.id = "p1".into();
        store.insert(pack).unwrap();
        store.add_term("p1", "Zephyr", &["zefir".into()]).unwrap();
        let mut off = UserPackStore::new_pack("Off", "");
        off.id = "p2".into();
        off.enabled = false;
        store.insert(off).unwrap();
        store.add_term("p2", "Quantum", &["kwantum".into()]).unwrap();
        let terms = store.terms_for();
        assert_eq!(terms.len(), 1);
        assert_eq!(terms[0].canonical, "zephyr");
        assert_eq!(terms[0].alias, "zefir");
    }

    #[test]
    fn word_without_mishearings_yields_self_pair() {
        let mut store = UserPackStore::default();
        let pack = UserPackStore::new_pack("P", "");
        let id = pack.id.clone();
        store.insert(pack).unwrap();
        store.add_term(&id, "Docker", &[]).unwrap();
        let terms = store.terms_for();
        assert_eq!(terms, vec![crate::vocab::PackTerm { canonical: "docker".into(), alias: "docker".into() }]);
    }
}
