use super::*;

pub(super) struct PreferencePaths {
    paths: [String; 4],
}

impl PreferencePaths {
    #[cfg(feature = "maintainer-native-qualification")]
    pub(super) fn for_fresh_tsf(service: GUID, english: GUID) -> Self {
        Self::from_root(
            format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{{{service:?}}}"),
            english,
        )
    }

    fn from_root(root: String, profile: GUID) -> Self {
        let profiles = format!("{root}\\LanguageProfile");
        let language = format!("{profiles}\\0x00000409");
        let leaf = format!("{language}\\{{{profile:?}}}");
        Self {
            paths: [root, profiles, language, leaf],
        }
    }

    fn validate_tree(&self, tree: &Value, enable: Option<bool>) -> Result<()> {
        let mut node = tree;
        for depth in 0..self.paths.len() {
            let values = node["values"]
                .as_object()
                .ok_or("residue values unavailable")?;
            let children = node["children"]
                .as_object()
                .ok_or("residue children unavailable")?;
            if node["present"] == false && values.is_empty() && children.is_empty() {
                return Ok(());
            }
            if node["present"] != true || enable.is_none() {
                return Err("unexpected disposable preference residue".into());
            }
            if depth == self.paths.len() - 1 {
                let expected = json!({"enable":registry::dword(u32::from(enable.unwrap()))});
                if !children.is_empty() || (!values.is_empty() && node["values"] != expected) {
                    return Err("disposable Enable residue differs".into());
                }
                return Ok(());
            }
            if !values.is_empty() {
                return Err("unexpected disposable ancestor values".into());
            }
            if children.is_empty() {
                return Ok(());
            }
            let name = self.paths[depth + 1]
                .rsplit_once('\\')
                .unwrap()
                .1
                .to_lowercase();
            if children.len() != 1 || !children.contains_key(&name) {
                return Err("unexpected disposable preference children".into());
            }
            node = &children[&name];
        }
        unreachable!()
    }

    fn observe(
        &self,
        view: REG_SAM_FLAGS,
        enable: Option<bool>,
        record: &mut impl FnMut(&str, &Value) -> Result<()>,
    ) -> Result<()> {
        let tree = registry_tree(HKEY_CURRENT_USER, view, &self.paths[0], 0, &mut 16);
        record(
            "cleanup-residue-readback",
            &json!({"view":view.0,"tree":tree.as_ref().ok(),"error":tree.as_ref().err().map(ToString::to_string)}),
        )?;
        let validation = self.validate_tree(&tree?, enable);
        record(
            "cleanup-residue-validation",
            &json!({"view":view.0,"error":validation.as_ref().err().map(ToString::to_string)}),
        )?;
        validation
    }

    pub(super) fn remove(
        &self,
        enable: Option<bool>,
        record: &mut impl FnMut(&str, &Value) -> Result<()>,
    ) -> Result<()> {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            self.observe(view, enable, record)?;
        }
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            self.observe(view, enable, record)?;
            if !registry::exists(HKEY_CURRENT_USER, view, &self.paths[0])? {
                continue;
            }
            let leaf = &self.paths[3];
            let value = registry::read(HKEY_CURRENT_USER, view, leaf, "Enable");
            record(
                "cleanup-enable-readback",
                &json!({"view":view.0,"value":value.as_ref().ok(),"error":value.as_ref().err().map(ToString::to_string)}),
            )?;
            let value = value?;
            if !value.is_null() {
                if Some(value) != enable.map(|enabled| registry::dword(u32::from(enabled))) {
                    return Err("disposable Enable changed before deletion".into());
                }
                let deleted =
                    registry::write(HKEY_CURRENT_USER, view, leaf, "Enable", &Value::Null);
                record(
                    "cleanup-enable-delete",
                    &json!({"view":view.0,"error":deleted.as_ref().err().map(ToString::to_string)}),
                )?;
                deleted?;
                let value = registry::read(HKEY_CURRENT_USER, view, leaf, "Enable");
                record(
                    "cleanup-enable-cleared-readback",
                    &json!({"view":view.0,"value":value.as_ref().ok(),"error":value.as_ref().err().map(ToString::to_string)}),
                )?;
                if !value?.is_null() {
                    return Err("disposable Enable remains after deletion".into());
                }
            }
            for path in self.paths.iter().rev() {
                self.observe(view, enable, record)?;
                let deleted = registry::remove_empty_key(HKEY_CURRENT_USER, view, path);
                record(
                    "cleanup-empty-key-delete",
                    &json!({"view":view.0,"path":path,"error":deleted.as_ref().err().map(ToString::to_string)}),
                )?;
                deleted?;
            }
            if registry::exists(HKEY_CURRENT_USER, view, &self.paths[0])? {
                return Err("disposable preference cleanup incomplete".into());
            }
        }
        Ok(())
    }
}

struct Isolated {
    preference: PreferencePaths,
    sentinel: String,
}

impl Isolated {
    fn new() -> Result<Self> {
        // SAFETY: GUID allocation creates no registration or platform editing state.
        let (root, profile) = unsafe { (CoCreateGuid()?, CoCreateGuid()?) };
        let enclosing = format!("Software\\KeyrillTests\\Qualification-{root:?}");
        let preference = PreferencePaths::from_root(format!("{enclosing}\\Fixture"), profile);
        let sentinel = format!("{enclosing}\\Sentinel");
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if registry::exists(HKEY_CURRENT_USER, view, &enclosing)? {
                return Err("isolated qualification test identity already exists".into());
            }
        }
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            registry::write(
                HKEY_CURRENT_USER,
                view,
                &sentinel,
                "unchanged",
                &registry::dword(42),
            )?;
        }
        Ok(Self {
            preference,
            sentinel,
        })
    }

    fn finish(self) {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            assert_eq!(
                registry::read(HKEY_CURRENT_USER, view, &self.sentinel, "unchanged").unwrap(),
                registry::dword(42)
            );
        }
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            registry::write(
                HKEY_CURRENT_USER,
                view,
                &self.preference.paths[3],
                "Enable",
                &Value::Null,
            )
            .unwrap();
            for path in self.preference.paths.iter().rev() {
                registry::remove_empty_key(HKEY_CURRENT_USER, view, path).unwrap();
            }
            registry::write(
                HKEY_CURRENT_USER,
                view,
                &self.sentinel,
                "unchanged",
                &Value::Null,
            )
            .unwrap();
            registry::remove_empty_key(HKEY_CURRENT_USER, view, &self.sentinel).unwrap();
            let enclosing = self.sentinel.rsplit_once('\\').unwrap().0;
            registry::remove_empty_key(HKEY_CURRENT_USER, view, enclosing).unwrap();
        }
    }

    fn snapshot(&self) -> Vec<Value> {
        [KEY_WOW64_64KEY, KEY_WOW64_32KEY]
            .into_iter()
            .map(|view| {
                registry_tree(
                    HKEY_CURRENT_USER,
                    view,
                    &self.preference.paths[0],
                    0,
                    &mut 32,
                )
                .unwrap()
            })
            .collect()
    }
}

#[test]
fn isolated_native_preference_cleanup_requires_complete_absence_for_both_dwords() {
    for enable in [false, true] {
        let fixture = Isolated::new().unwrap();
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            registry::write(
                HKEY_CURRENT_USER,
                view,
                &fixture.preference.paths[3],
                "Enable",
                &registry::dword(u32::from(enable)),
            )
            .unwrap();
        }
        let root = fixture.preference.paths[0].clone();
        let outcome = fixture.preference.remove(Some(enable), &mut |_, _| Ok(()));
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if outcome.is_ok() {
                assert!(!registry::exists(HKEY_CURRENT_USER, view, &root).unwrap());
            }
            assert_eq!(
                registry::read(HKEY_CURRENT_USER, view, &fixture.sentinel, "unchanged").unwrap(),
                registry::dword(42)
            );
        }
        fixture.finish();
        outcome.unwrap();
    }
}

#[test]
fn isolated_native_preference_cleanup_accepts_absence_and_each_empty_prefix() {
    let absent = Isolated::new().unwrap();
    absent
        .preference
        .remove(None, &mut |label, _| {
            assert!(!label.ends_with("delete"));
            Ok(())
        })
        .unwrap();
    absent.finish();
    for depth in 0..4 {
        let fixture = Isolated::new().unwrap();
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            let path = &fixture.preference.paths[depth];
            registry::write(
                HKEY_CURRENT_USER,
                view,
                path,
                "create-empty",
                &registry::dword(0),
            )
            .unwrap();
            registry::write(HKEY_CURRENT_USER, view, path, "create-empty", &Value::Null).unwrap();
        }
        let before = fixture.snapshot();
        assert!(
            fixture
                .preference
                .remove(None, &mut |label, _| {
                    assert!(!label.ends_with("delete"));
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(fixture.snapshot(), before);
        fixture
            .preference
            .remove(Some(false), &mut |_, _| Ok(()))
            .unwrap();
        assert!(
            fixture
                .snapshot()
                .iter()
                .all(|tree| tree["present"] == false)
        );
        fixture.finish();
    }
}

#[test]
fn isolated_native_preference_cleanup_refuses_extra_values_and_wrong_enable_bytes() {
    for (depth, name, value) in [
        (0, "extra", registry::dword(42)),
        (1, "extra", registry::dword(42)),
        (2, "extra", registry::dword(42)),
        (3, "extra", registry::dword(42)),
        (3, "Enable", registry::dword(1)),
        (3, "Enable", registry::string("0")),
        (3, "Enable", json!({"type":REG_DWORD.0,"data":[0]})),
    ] {
        let fixture = Isolated::new().unwrap();
        let path = &fixture.preference.paths[depth];
        registry::write(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path, name, &value).unwrap();
        let before = fixture.snapshot();
        assert!(
            fixture
                .preference
                .remove(Some(false), &mut |label, _| {
                    assert!(!label.ends_with("delete"));
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(fixture.snapshot(), before);
        registry::write(HKEY_CURRENT_USER, KEY_WOW64_64KEY, path, name, &Value::Null).unwrap();
        fixture.finish();
    }
}

#[test]
fn isolated_native_preference_cleanup_refuses_foreign_branches_before_any_delete() {
    for (depth, child) in [
        (0, "Other"),
        (1, "0x00000419"),
        (2, "OtherProfile"),
        (3, "Child"),
    ] {
        let fixture = Isolated::new().unwrap();
        let path = format!("{}\\{child}", fixture.preference.paths[depth]);
        registry::write(
            HKEY_CURRENT_USER,
            KEY_WOW64_64KEY,
            &path,
            "extra",
            &registry::dword(42),
        )
        .unwrap();
        let before = fixture.snapshot();
        assert!(
            fixture
                .preference
                .remove(Some(false), &mut |label, _| {
                    assert!(!label.ends_with("delete"));
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(fixture.snapshot(), before);
        registry::write(
            HKEY_CURRENT_USER,
            KEY_WOW64_64KEY,
            &path,
            "extra",
            &Value::Null,
        )
        .unwrap();
        registry::remove_empty_key(HKEY_CURRENT_USER, KEY_WOW64_64KEY, &path).unwrap();
        fixture.finish();
    }
}

#[test]
fn isolated_native_preference_cleanup_preflights_second_view_before_any_delete() {
    let fixture = Isolated::new().unwrap();
    let root = &fixture.preference.paths[0];
    let mut injected = false;
    let outcome = fixture.preference.remove(Some(false), &mut |label, value| {
        assert!(!label.ends_with("delete"));
        if label == "cleanup-residue-readback" && value["view"] == KEY_WOW64_64KEY.0 {
            assert!(!injected);
            registry::write(
                HKEY_CURRENT_USER,
                KEY_WOW64_32KEY,
                root,
                "late-extra",
                &registry::dword(42),
            )?;
            injected = true;
        }
        Ok(())
    });
    assert!(injected && outcome.is_err());
    assert_eq!(
        registry::read(HKEY_CURRENT_USER, KEY_WOW64_32KEY, root, "late-extra").unwrap(),
        registry::dword(42)
    );
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_32KEY,
        root,
        "late-extra",
        &Value::Null,
    )
    .unwrap();
    fixture.finish();
}

#[test]
fn isolated_native_preference_cleanup_records_empty_key_refusal_and_stops() {
    let fixture = Isolated::new().unwrap();
    let leaf = &fixture.preference.paths[3];
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        leaf,
        "Enable",
        &registry::dword(0),
    )
    .unwrap();
    let mut validations = 0;
    let mut effects = Vec::new();
    let outcome = fixture.preference.remove(Some(false), &mut |label, value| {
        if label == "cleanup-residue-validation" {
            validations += 1;
            if validations == 4 {
                registry::write(
                    HKEY_CURRENT_USER,
                    KEY_WOW64_64KEY,
                    leaf,
                    "late-extra",
                    &registry::dword(42),
                )?;
            }
        }
        if label.ends_with("delete") {
            effects.push((label.to_owned(), value.clone()));
        }
        Ok(())
    });
    assert!(outcome.is_err());
    assert_eq!(effects.len(), 2);
    assert_eq!(effects.last().unwrap().0, "cleanup-empty-key-delete");
    assert_eq!(
        effects.last().unwrap().1["error"],
        "disposable registry key is not empty"
    );
    assert!(registry::exists(HKEY_CURRENT_USER, KEY_WOW64_64KEY, leaf).unwrap());
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        leaf,
        "late-extra",
        &Value::Null,
    )
    .unwrap();
    fixture.finish();
}

#[test]
fn isolated_native_preference_cleanup_preserves_an_older_fixture_and_stops_on_evidence_failure() {
    let old = Isolated::new().unwrap();
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &old.preference.paths[3],
        "Enable",
        &registry::dword(1),
    )
    .unwrap();
    let before = old.snapshot();
    let fixture = Isolated::new().unwrap();
    registry::write(
        HKEY_CURRENT_USER,
        KEY_WOW64_64KEY,
        &fixture.preference.paths[3],
        "Enable",
        &registry::dword(0),
    )
    .unwrap();
    let own_before = fixture.snapshot();
    assert!(
        fixture
            .preference
            .remove(Some(false), &mut |label, _| {
                if label == "cleanup-enable-readback" {
                    return Err("injected evidence failure".into());
                }
                assert!(!label.ends_with("delete"));
                Ok(())
            })
            .is_err()
    );
    assert_eq!(fixture.snapshot(), own_before);
    fixture
        .preference
        .remove(Some(false), &mut |_, _| Ok(()))
        .unwrap();
    assert_eq!(old.snapshot(), before);
    fixture.finish();
    old.finish();
}
