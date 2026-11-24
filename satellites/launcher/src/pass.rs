//! The vault card (`seiza-launcher --pass`): a read-only view of a KeePass vault. The model here has no
//! Wayland in it, so unlock, grouping and find are tested anywhere. All crypto is the `keepass` crate's.

use std::path::Path;

use keepass::{
    Database, DatabaseKey,
    db::{EntryRef, GroupRef, fields},
};

/// The space of an entry that sits directly under the vault's root.
const LOOSE: &str = "other";

#[derive(Clone, Debug)]
pub struct Login {
    pub title: String,
    pub user: String,
    pub url: String,
    pub space: String,
    password: String,
}

impl Login {
    pub fn password(&self) -> &str {
        &self.password
    }
}

/// The lowercased host of a url or bare address, without scheme, port, path or `www.`; empty if none.
pub fn host(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("");
    let h = h.rsplit_once('@').map_or(h, |(_, h)| h);
    let h = h.split(':').next().unwrap_or("").to_lowercase();
    h.strip_prefix("www.").unwrap_or(&h).to_string()
}

/// Read every login of the vault at `path`. Errors are readable and never contain the passphrase.
pub fn open(path: &Path, passphrase: &str) -> Result<Vec<Login>, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    let db = Database::open(&mut file, DatabaseKey::new().with_password(passphrase))
        .map_err(|_| format!("can't unlock {} (wrong passphrase, or not a kdbx vault)", path.display()))?;
    let mut out = Vec::new();
    let root = db.root();
    for e in root.entries() {
        out.push(login(&e, LOOSE));
    }
    for g in root.groups() {
        walk(&g, &g.name, &mut out);
    }
    Ok(out)
}

/// Every entry under `g`, nested groups included, all filed under the top-level `space`.
fn walk(g: &GroupRef<'_>, space: &str, out: &mut Vec<Login>) {
    out.extend(g.entries().map(|e| login(&e, space)));
    for sub in g.groups() {
        walk(&sub, space, out);
    }
}

fn login(e: &EntryRef<'_>, space: &str) -> Login {
    let get = |k: &str| e.get(k).unwrap_or("").to_string();
    Login { title: get(fields::TITLE), user: get(fields::USERNAME), url: get(fields::URL), space: space.to_string(), password: get(fields::PASSWORD) }
}

/// One row of the card: every login of a site within a space.
#[derive(Debug, PartialEq)]
pub struct Row {
    pub host: String,
    pub space: String,
    pub logins: Vec<usize>,
}

/// Fold logins into one row per (space, host); a login without a host is a row of its own, named by its title.
pub fn fold(logins: &[Login]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for (i, l) in logins.iter().enumerate() {
        let h = host(&l.url);
        match rows.iter_mut().find(|r| !h.is_empty() && r.space == l.space && r.host == h) {
            Some(r) => r.logins.push(i),
            None => {
                let name = if !h.is_empty() { h } else if l.title.is_empty() { "(untitled)".into() } else { l.title.to_lowercase() };
                rows.push(Row { host: name, space: l.space.clone(), logins: vec![i] });
            }
        }
    }
    rows.sort_by(|a, b| (&a.space, &a.host).cmp(&(&b.space, &b.host)));
    rows
}

/// Indices into `rows` matching `query` (case-insensitive substring of title, user, host or space).
pub fn find(logins: &[Login], rows: &[Row], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    (0..rows.len())
        .filter(|&i| {
            let r = &rows[i];
            q.is_empty()
                || r.host.contains(&q)
                || r.space.to_lowercase().contains(&q)
                || r.logins.iter().any(|&j| logins[j].title.to_lowercase().contains(&q) || logins[j].user.to_lowercase().contains(&q))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use keepass::db::GroupMut;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PASS: &str = "correct horse battery";

    fn vault() -> std::path::PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!("seiza-pass-test-{}-{}.kdbx", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let mut db = Database::new();
        let mut root = db.root_mut();
        let add = |g: &mut GroupMut<'_>, title: &str, user: &str, url: &str, pw: &str| {
            let mut e = g.add_entry();
            e.set_unprotected(fields::TITLE, title);
            e.set_unprotected(fields::USERNAME, user);
            e.set_unprotected(fields::URL, url);
            e.set_protected(fields::PASSWORD, pw);
        };
        let mut socials = root.add_group();
        socials.name = "socials".into();
        add(&mut socials, "gmail a", "a@x.org", "https://Mail.Google.com/x", "pw-a");
        add(&mut socials, "gmail b", "b@x.org", "mail.google.com", "pw-b");
        let mut deep = socials.add_group();
        deep.name = "old".into();
        add(&mut deep, "forum", "me", "https://forum.example/login", "pw-f");
        let mut fin = root.add_group();
        fin.name = "finances".into();
        add(&mut fin, "bank", "me", "https://www.bank.example:8443/", "");
        add(&mut fin, "", "nourl", "", "pw-n");
        add(&mut root, "loose", "l", "", "pw-l");
        let mut f = std::fs::File::create(&path).unwrap();
        db.save(&mut f, DatabaseKey::new().with_password(PASS)).unwrap();
        path
    }

    #[test]
    fn host_strips_scheme_path_www_and_case() {
        assert_eq!(host("https://Mail.Google.com/x?y#z"), "mail.google.com");
        assert_eq!(host("www.Bank.example:8443/a"), "bank.example");
        assert_eq!(host("user@host.org"), "host.org");
        assert_eq!(host(""), "");
    }

    #[test]
    fn open_lists_logins_with_top_level_space() {
        let l = open(&vault(), PASS).unwrap();
        assert_eq!(l.len(), 6);
        let by = |t: &str| l.iter().find(|x| x.title == t).unwrap();
        assert_eq!(by("gmail a").space, "socials");
        assert_eq!(by("forum").space, "socials", "nested group belongs to its top-level space");
        assert_eq!(by("loose").space, "other");
        assert_eq!(by("gmail a").password(), "pw-a");
        assert_eq!(by("bank").password(), "", "an empty password stays listed");
    }

    #[test]
    fn open_wrong_passphrase_is_err_without_passphrase_in_message() {
        let e = open(&vault(), "nope-secret-guess").unwrap_err();
        assert!(e.contains("can't unlock"), "{e}");
        assert!(!e.contains("nope-secret-guess"), "{e}");
    }

    #[test]
    fn open_missing_file_names_the_path() {
        let e = open(Path::new("/nonexistent/v.kdbx"), PASS).unwrap_err();
        assert!(e.contains("/nonexistent/v.kdbx"), "{e}");
    }

    #[test]
    fn fold_merges_same_host_in_same_space() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        let g = rows.iter().find(|r| r.host == "mail.google.com").unwrap();
        assert_eq!((g.space.as_str(), g.logins.len()), ("socials", 2));
    }

    #[test]
    fn fold_keeps_urlless_entries_separate() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        assert!(rows.iter().any(|r| r.space == "other" && r.logins.len() == 1));
        assert!(rows.iter().any(|r| r.space == "finances" && r.host == "(untitled)"), "an untitled, urlless login still has a row");
        assert_eq!(rows.iter().map(|r| r.logins.len()).sum::<usize>(), 6);
    }

    #[test]
    fn find_matches_title_user_host_space_case_insensitively() {
        let l = open(&vault(), PASS).unwrap();
        let rows = fold(&l);
        let hits = |q: &str| find(&l, &rows, q).len();
        assert_eq!(hits(""), rows.len());
        assert_eq!(hits("GMAIL"), 1);
        assert_eq!(hits("b@x"), 1);
        assert_eq!(hits("finances"), 2);
        assert_eq!(hits("zzz"), 0);
    }
}
