use super::super::super::profile_repair::Profile;
use super::super::super::{Installation, files};
use super::qualification_cleanup::PreferencePaths;
use super::*;
use keyrill_windows_build::Digest;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
use windows_core::Interface;

#[derive(Clone, Copy)]
struct Disposable {
    service: GUID,
    profiles: [GUID; 2],
}
impl Disposable {
    fn new() -> Result<Self> {
        // SAFETY: these calls allocate identities without registering native resources.
        let ids = unsafe { [CoCreateGuid()?, CoCreateGuid()?, CoCreateGuid()?] };
        for (index, id) in ids.iter().enumerate() {
            if *id == SERVICE_ID
                || PROFILES.iter().any(|p| p.id == *id)
                || ids[..index].contains(id)
            {
                return Err("disposable GUID collision".into());
            }
        }
        Ok(Self {
            service: ids[0],
            profiles: [ids[1], ids[2]],
        })
    }
    fn tip(self) -> String {
        format!("SOFTWARE\\Microsoft\\CTF\\TIP\\{{{:?}}}", self.service)
    }
    fn com(self) -> String {
        format!("SOFTWARE\\Classes\\CLSID\\{{{:?}}}", self.service)
    }
    fn profile(self, index: usize) -> String {
        format!(
            "{}\\LanguageProfile\\0x{:08x}\\{{{:?}}}",
            self.tip(),
            PROFILES[index].language,
            self.profiles[index]
        )
    }
    fn require_absent(self) -> Result<()> {
        for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                for path in [self.tip(), self.com()] {
                    if registry::exists(hive, view, &path)? {
                        return Err("disposable native identity is already registered or cleanup incomplete".into());
                    }
                }
            }
        }
        Ok(())
    }
}

struct FreshDisposable {
    ids: Disposable,
    preference: PreferencePaths,
}

impl FreshDisposable {
    fn new() -> Result<Self> {
        let ids = Disposable::new()?;
        ids.require_absent()?;
        Ok(Self {
            ids,
            preference: PreferencePaths::for_fresh_tsf(ids.service, ids.profiles[0]),
        })
    }

    fn cleanup(
        self,
        enable: Option<bool>,
        verified: &Value,
        protected: &Value,
        installation: &mut Installation,
        manager: &ITfInputProcessorProfiles,
        evidence: &mut Evidence,
    ) -> Result<()> {
        let before =
            evidence.readback("before-cleanup", disposable_observation(self.ids, manager))?;
        let current = evidence.readback(
            "before-cleanup-protected",
            protected_observation(installation, manager),
        )?;
        if before != *verified || current != *protected {
            return Err("qualification state changed before cleanup; fixture retained".into());
        }
        evidence.record(
            "cleanup-start",
            &json!({"service":format!("{:?}",self.ids.service)}),
        )?;
        for (index, profile) in PROFILES.iter().enumerate() {
            // SAFETY: this consuming owner carries the current invocation's initial absence.
            let result = unsafe {
                (manager.vtable().RemoveLanguageProfile)(
                    manager.as_raw(),
                    &self.ids.service,
                    profile.language,
                    &self.ids.profiles[index],
                )
            };
            evidence.record(
                "cleanup-remove-profile",
                &json!({"index":index,"hresult":result.0}),
            )?;
            if result.is_err() {
                let _ = evidence.readback(
                    "failed-cleanup-readback",
                    disposable_observation(self.ids, manager),
                );
                let _ = evidence.readback(
                    "failed-cleanup-protected",
                    protected_observation(installation, manager),
                );
                return Err(windows_core::Error::from_hresult(result).into());
            }
        }
        // SAFETY: this service is the same fresh disposable owner, with no teardown retry.
        let result = unsafe { (manager.vtable().Unregister)(manager.as_raw(), &self.ids.service) };
        evidence.record("cleanup-unregister", &json!({"hresult":result.0}))?;
        let removed = evidence.readback(
            "after-native-cleanup",
            disposable_observation(self.ids, manager),
        );
        let current = evidence.readback(
            "after-native-cleanup-protected",
            protected_observation(installation, manager),
        );
        result.ok()?;
        let removed = removed?;
        if current? != *protected {
            return Err("production or source state changed during native cleanup".into());
        }
        for tree in removed["trees"].as_array().unwrap() {
            if tree["com"]["present"] != false
                || (tree["hive"] == HKEY_LOCAL_MACHINE.0 as usize
                    && tree["tip"]["present"] != false)
            {
                return Err(
                    "unexpected disposable registration remains after native cleanup".into(),
                );
            }
        }
        let cleanup = self
            .preference
            .remove(enable, &mut |label, value| evidence.record(label, value));
        let disposable = evidence.readback(
            "after-cleanup-disposable",
            disposable_observation(self.ids, manager),
        );
        let cleaned = evidence.readback(
            "after-cleanup",
            protected_observation(installation, manager),
        );
        cleanup?;
        disposable?;
        self.ids.require_absent()?;
        if *protected != cleaned? {
            return Err("production or source state changed after disposable cleanup".into());
        }
        evidence.record(
            "case-passed",
            &json!({"service":format!("{:?}",self.ids.service),"disposable_absent":true}),
        )?;
        Ok(())
    }
}

struct Forwarding {
    native: ITfInputProcessorProfiles,
    ids: Disposable,
    calls: Cell<usize>,
    result: Cell<Option<i32>>,
}
#[implement(ITfInputProcessorProfiles)]
struct Forwarder(Rc<Forwarding>);

macro_rules! reject {
    ($(fn $name:ident($($arg:ident: $ty:ty),*) -> $result:ty;)*) => {
        $(fn $name(&self, $($arg: $ty),*) -> windows_core::Result<$result> {
            $(let _ = $arg;)*
            Err(E_FAIL.into())
        })*
    };
}
impl ITfInputProcessorProfiles_Impl for Forwarder_Impl {
    fn AddLanguageProfile(
        &self,
        class: *const GUID,
        language: u16,
        profile: *const GUID,
        description: &PCWSTR,
        description_len: u32,
        icon: &PCWSTR,
        icon_len: u32,
        index: u32,
    ) -> windows_core::Result<()> {
        if class.is_null() || profile.is_null() || description.is_null() || icon.is_null() {
            return Err(E_FAIL.into());
        }
        // SAFETY: only profile_write_with supplies the live immutable GUIDs and buffers.
        let expected = unsafe { *class == SERVICE_ID && *profile == PROFILES[0].id };
        self.0.calls.set(self.0.calls.get() + 1);
        if !expected || language != PROFILES[0].language || index != 0 || self.0.calls.get() != 1 {
            return Err(E_FAIL.into());
        }
        // SAFETY: translate only the validated production tuple. Original pointers and counts
        // remain untouched, exercising the writer's actual storage through the native call.
        let result = unsafe {
            (self.0.native.vtable().AddLanguageProfile)(
                self.0.native.as_raw(),
                &self.0.ids.service,
                language,
                &self.0.ids.profiles[0],
                *description,
                description_len,
                *icon,
                icon_len,
                index,
            )
        };
        self.0.result.set(Some(result.0));
        result.ok()
    }
    reject! {
        fn Register(class: *const GUID) -> ();
        fn Unregister(class: *const GUID) -> ();
        fn RemoveLanguageProfile(class: *const GUID, language: u16, profile: *const GUID) -> ();
        fn EnumInputProcessorInfo() -> IEnumGUID;
        fn GetDefaultLanguageProfile(language: u16, category: *const GUID, class: *mut GUID, profile: *mut GUID) -> ();
        fn SetDefaultLanguageProfile(language: u16, class: *const GUID, profile: *const GUID) -> ();
        fn ActivateLanguageProfile(class: *const GUID, language: u16, profile: *const GUID) -> ();
        fn GetActiveLanguageProfile(class: *const GUID, language: *mut u16, profile: *mut GUID) -> ();
        fn GetLanguageProfileDescription(class: *const GUID, language: u16, profile: *const GUID) -> BSTR;
        fn GetCurrentLanguage() -> u16;
        fn ChangeCurrentLanguage(language: u16) -> ();
        fn GetLanguageList(languages: *mut *mut u16, count: *mut u32) -> ();
        fn EnumLanguageProfiles(language: u16) -> IEnumTfLanguageProfiles;
        fn EnableLanguageProfile(class: *const GUID, language: u16, profile: *const GUID, enabled: BOOL) -> ();
        fn IsEnabledLanguageProfile(class: *const GUID, language: u16, profile: *const GUID) -> BOOL;
        fn EnableLanguageProfileByDefault(class: *const GUID, language: u16, profile: *const GUID, enabled: BOOL) -> ();
        fn SubstituteKeyboardLayout(class: *const GUID, language: u16, profile: *const GUID, layout: HKL) -> ();
    }
}

struct Evidence(File);
impl Evidence {
    fn record(&mut self, label: &str, value: &Value) -> Result<()> {
        writeln!(self.0, "{}", json!({"cut":label,"observation":value}))?;
        self.0.sync_all()?;
        Ok(())
    }

    fn readback(&mut self, label: &str, result: Result<Value>) -> Result<Value> {
        self.record(label, &json!({"readback":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)}))?;
        result
    }
}

fn native_profile(
    manager: &ITfInputProcessorProfiles,
    service: GUID,
    profile: GUID,
    language: u16,
) -> Result<Value> {
    let substitute: ITfInputProcessorProfileSubstituteLayout = manager.cast()?;
    let mut description = BSTR::new();
    let mut enabled = BOOL(0);
    let mut layout = HKL::default();
    // SAFETY: these read-only queries retain COM owners, GUIDs and separate output storage.
    let results = unsafe {
        [
            (manager.vtable().GetLanguageProfileDescription)(
                manager.as_raw(),
                &service,
                language,
                &profile,
                (&mut description as *mut BSTR).cast(),
            ),
            (manager.vtable().IsEnabledLanguageProfile)(
                manager.as_raw(),
                &service,
                language,
                &profile,
                &mut enabled,
            ),
            (substitute.vtable().GetSubstituteKeyboardLayout)(
                substitute.as_raw(),
                &service,
                language,
                &profile,
                &mut layout,
            ),
        ]
    };
    Ok(
        json!({"hresults":results.map(|hr|hr.0),"description":description.to_vec(),"enabled":enabled.0,"layout":layout.0 as usize}),
    )
}

fn source_observation(manager: &ITfInputProcessorProfiles) -> Result<Value> {
    let profiles: ITfInputProcessorProfileMgr = manager.cast()?;
    let mut active = TF_INPUTPROCESSORPROFILE::default();
    let mut language = 0;
    let mut layouts = [HKL::default(); 256];
    // SAFETY: query-only APIs retain COM owners and write to distinct bounded storage.
    let (active_hr, language_hr, count, foreground, thread, foreground_layout, owner_layout) = unsafe {
        let active_hr = (profiles.vtable().GetActiveProfile)(
            profiles.as_raw(),
            &GUID_TFCAT_TIP_KEYBOARD,
            &mut active,
        );
        let language_hr = (manager.vtable().GetCurrentLanguage)(manager.as_raw(), &mut language);
        let count = GetKeyboardLayoutList(Some(&mut layouts));
        let foreground = GetForegroundWindow();
        let thread = GetWindowThreadProcessId(foreground, None);
        (
            active_hr,
            language_hr,
            count,
            foreground,
            thread,
            GetKeyboardLayout(thread),
            GetKeyboardLayout(0),
        )
    };
    if count <= 0
        || count as usize >= layouts.len()
        || foreground.is_invalid()
        || thread == 0
        || owner_layout.0.is_null()
        || foreground_layout.0.is_null()
        || language == 0
        || active_hr.is_err()
        || language_hr.is_err()
    {
        return Err("source observation unavailable".into());
    }
    Ok(
        json!({"scope":"test owner thread active TSF profile and foreground thread keyboard layout",
        "active_hresult":active_hr.0,"language_hresult":language_hr.0,"language":language,
        "loaded":layouts[..count as usize].iter().map(|layout|layout.0 as usize).collect::<Vec<_>>(),
        "owner_layout":owner_layout.0 as usize,"foreground":foreground.0 as usize,"foreground_thread":thread,
        "foreground_layout":foreground_layout.0 as usize,
        "active":{"type":active.dwProfileType,"language":active.langid,"class":format!("{:?}",active.clsid),
        "profile":format!("{:?}",active.guidProfile),"category":format!("{:?}",active.catid),
        "substitute":active.hklSubstitute.0 as usize,"caps":active.dwCaps,"layout":active.hkl.0 as usize,"flags":active.dwFlags}}),
    )
}

fn disposable_observation(ids: Disposable, manager: &ITfInputProcessorProfiles) -> Result<Value> {
    let mut trees = Vec::new();
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            trees.push(json!({"hive":hive.0 as usize,"view":view.0,
                "tip":registry_tree(hive,view,&ids.tip(),0,&mut 128)?,
                "com":registry_tree(hive,view,&ids.com(),0,&mut 128)?}));
        }
    }
    Ok(
        json!({"trees":trees,"profiles":[native_profile(manager,ids.service,ids.profiles[0],PROFILES[0].language)?,native_profile(manager,ids.service,ids.profiles[1],PROFILES[1].language)?]}),
    )
}

fn file_observation(path: &Path, depth: usize, budget: &mut usize) -> Result<Value> {
    if depth > 8 || *budget == 0 {
        return Err("qualification file snapshot exceeded bound".into());
    }
    *budget -= 1;
    if !path.try_exists()? {
        return Ok(Value::Null);
    }
    files::ordinary(path)?;
    let metadata = fs::metadata(path)?;
    if metadata.is_file() {
        if metadata.len() > 256 * 1024 * 1024 {
            return Err("qualification file exceeds bound".into());
        }
        return Ok(
            json!({"size":metadata.len(),"sha256":Digest::of_bytes(&fs::read(path)?).to_string()}),
        );
    }
    let mut children = serde_json::Map::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "non-Unicode qualification file")?;
        children.insert(name, file_observation(&entry.path(), depth + 1, budget)?);
    }
    Ok(json!({"directory":children}))
}

fn protected_observation(
    installation: &mut Installation,
    manager: &ITfInputProcessorProfiles,
) -> Result<Value> {
    Ok(
        json!({"repair_owner":installation.repair_snapshot(Profile::English)?,
        "files":file_observation(&installation.root,0,&mut 512)?,
        "sources":source_observation(manager)?}),
    )
}

fn profile_metadata(ids: Disposable) -> Result<Vec<Value>> {
    [KEY_WOW64_64KEY, KEY_WOW64_32KEY]
        .into_iter()
        .map(|view| {
            let mut values = serde_json::Map::new();
            for name in ["Description", "IconFile", "IconIndex"] {
                values.insert(
                    name.into(),
                    registry::read(HKEY_LOCAL_MACHINE, view, &ids.profile(0), name)?,
                );
            }
            Ok(Value::Object(values))
        })
        .collect()
}

fn without_metadata(value: &Value, ids: Disposable) -> Value {
    let mut protected = value.clone();
    for tree in protected["trees"].as_array_mut().unwrap() {
        if tree["hive"] != HKEY_LOCAL_MACHINE.0 as usize {
            continue;
        }
        let values = &mut tree["tip"]["children"]["languageprofile"]["children"]
            [format!("0x{:08x}", PROFILES[0].language)]["children"]
            [format!("{{{:?}}}", ids.profiles[0]).to_lowercase()]["values"];
        for name in ["description", "iconfile", "iconindex"] {
            values
                .as_object_mut()
                .expect("raw profile tree shape")
                .remove(name);
        }
    }
    protected["profiles"][0]
        .as_object_mut()
        .unwrap()
        .remove("description");
    protected
}

fn setup_case(
    manager: &ITfInputProcessorProfiles,
    ids: Disposable,
    enable: Option<bool>,
    machine: bool,
    substitute: bool,
    original_icon: &str,
) -> Result<()> {
    // SAFETY: all mutation tuples use this freshly generated and absence-checked identity.
    unsafe {
        manager.Register(&ids.service)?;
        for (index, profile) in PROFILES.iter().enumerate() {
            let description = profile_buffer(if index == 0 {
                "Keyrill English PUBLIC-CORRUPTED-SUFFIX"
            } else {
                "Keyrill public RU sibling"
            });
            let icon = profile_buffer(original_icon);
            manager.AddLanguageProfile(
                &ids.service,
                profile.language,
                &ids.profiles[index],
                &description[..description.len() - 1],
                &icon[..icon.len() - 1],
                0,
            )?;
        }
        if let Some(enabled) = enable {
            manager.EnableLanguageProfile(
                &ids.service,
                PROFILES[0].language,
                &ids.profiles[0],
                enabled,
            )?;
        }
        if substitute {
            manager.SubstituteKeyboardLayout(
                &ids.service,
                PROFILES[0].language,
                &ids.profiles[0],
                loaded_layout(0)?,
            )?;
        }
    }
    for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
        let root_enable = if machine {
            registry::string("1")
        } else {
            Value::Null
        };
        registry::write(HKEY_LOCAL_MACHINE, view, &ids.tip(), "Enable", &root_enable)?;
        if registry::read(HKEY_LOCAL_MACHINE, view, &ids.tip(), "Enable")? != root_enable {
            return Err("disposable machine Enable baseline differs".into());
        }
        if enable.is_none() {
            registry::write(
                HKEY_CURRENT_USER,
                view,
                &ids.profile(0),
                "Enable",
                &Value::Null,
            )?;
        }
        let observed = registry::read(HKEY_CURRENT_USER, view, &ids.profile(0), "Enable")?;
        if observed.is_null() != enable.is_none() {
            return Err("disposable Enable baseline unavailable".into());
        }
        let layout = registry::read(
            HKEY_LOCAL_MACHINE,
            view,
            &ids.profile(0),
            "SubstituteLayout",
        )?;
        if layout
            != if substitute {
                registry::string("0x04090409")
            } else {
                Value::Null
            }
        {
            return Err("disposable substitute baseline differs".into());
        }
    }
    if let Some(expected) = enable {
        // SAFETY: read-only query of the disposable tuple established immediately above.
        if unsafe {
            manager.IsEnabledLanguageProfile(
                &ids.service,
                PROFILES[0].language,
                &ids.profiles[0],
            )?
        }
        .as_bool()
            != expected
        {
            return Err("native disposable Enable baseline differs".into());
        }
    }
    Ok(())
}

pub(super) fn read_only_preflight() -> Result<()> {
    let elevation = files::require_elevated();
    println!(
        "Elevation query {}",
        json!({"elevated":elevation.is_ok(),"error":elevation.err().map(|error|error.to_string())})
    );
    let mut installation = Installation::open()?;
    let manager = profiles()?;
    let observation = protected_observation(&mut installation, &manager)?;
    println!(
        "Read-only protected snapshot SHA256 {}",
        Digest::of_bytes(&serde_json::to_vec(&observation)?)
    );
    let mut profiles = Vec::new();
    for profile in PROFILES {
        let native = native_profile(&manager, SERVICE_ID, profile.id, profile.language)?;
        println!(
            "Read-only profile language {} HRESULTs {}",
            profile.language, native["hresults"]
        );
        profiles.push(native);
    }
    require_native_observations(&json!({"profiles":profiles}))?;
    let layout = loaded_layout(0)?;
    println!("Already-loaded US layout {:#x}", layout.0 as usize);
    Ok(())
}

pub(super) fn run() -> Result<()> {
    if std::env::var("KEYRILL_MAINTAINER_NATIVE_PROFILE_QUALIFICATION").as_deref()
        != Ok("disposable-profile-qualification-v1")
    {
        return Err("native qualification requires explicit maintainer environment opt-in".into());
    }
    files::require_elevated()?;
    let mut installation = Installation::open()?;
    let manager = profiles()?;
    let run_id = Disposable::new()?;
    let path: PathBuf = std::env::temp_dir().join(format!(
        "keyrill-profile-qualification-{:?}.jsonl",
        run_id.service
    ));
    let mut evidence = Evidence(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?,
    );
    println!(
        "Native qualification evidence {}. Any failure retains the current disposable identity.",
        path.display()
    );
    evidence.record(
        "identity",
        &json!({"elevated":true,"bitness":usize::BITS,
        "user_sid":files::user_sid()?,
        "runtime_os_build":registry::read(HKEY_LOCAL_MACHINE,KEY_WOW64_64KEY,r"SOFTWARE\Microsoft\Windows NT\CurrentVersion","CurrentBuildNumber")?,
        "runtime_os_revision":registry::read(HKEY_LOCAL_MACHINE,KEY_WOW64_64KEY,r"SOFTWARE\Microsoft\Windows NT\CurrentVersion","UBR")?,
        "executable_sha256":Digest::of_bytes(&fs::read(std::env::current_exe()?)?).to_string()}),
    )?;
    let icon_directory =
        std::env::temp_dir().join(format!("keyrill-profile-icons-{:?}", run_id.service));
    fs::create_dir(&icon_directory)?;
    let original_icon = icon_directory.join("public-original.ico");
    let desired_icon = icon_directory.join("public-repaired-🔑.ico");
    for (path, russian) in [(&original_icon, false), (&desired_icon, true)] {
        let mut icon = OpenOptions::new().write(true).create_new(true).open(path)?;
        icon.write_all(&keyrill_windows_build::artifact::icon(russian))?;
        icon.sync_all()?;
    }
    let original_path = original_icon
        .to_str()
        .ok_or("non-Unicode qualification icon")?;
    let desired_path = desired_icon
        .to_str()
        .ok_or("non-Unicode qualification icon")?;
    evidence.record(
        "public-icon-fixtures",
        &json!({"original":original_path,"desired":desired_path}),
    )?;
    let mut cases = Vec::new();
    for enable in [None, Some(false), Some(true)] {
        for machine in [false, true] {
            for substitute in [false, true] {
                cases.push((enable, machine, substitute, PROFILES[0].description));
            }
        }
    }
    cases.push((Some(true), true, true, "Keyrill public 🔑"));
    for (enable, machine, substitute, description) in cases {
        let fresh = FreshDisposable::new()?;
        let ids = fresh.ids;
        evidence.record("case",&json!({"service":format!("{:?}",ids.service),
            "profiles":ids.profiles.map(|id|format!("{id:?}")),"enable":enable,"machine_enable":machine,
            "substitute":substitute,"description":description}))?;
        let protected = protected_observation(&mut installation, &manager)?;
        evidence.record("before-setup", &protected)?;
        let setup = setup_case(&manager, ids, enable, machine, substitute, original_path);
        evidence.record(
            "setup-result",
            &json!({"error":setup.as_ref().err().map(ToString::to_string)}),
        )?;
        if let Err(error) = setup {
            let readback = disposable_observation(ids, &manager);
            evidence.record("failed-setup-readback",&json!({"readback":readback.as_ref().ok(),"error":readback.err().map(|error|error.to_string())}))?;
            let protected = protected_observation(&mut installation, &manager);
            evidence.record("failed-setup-protected", &json!({"readback":protected.as_ref().ok(),"error":protected.as_ref().err().map(ToString::to_string)}))?;
            return Err(error);
        }
        let baseline = disposable_observation(ids, &manager)?;
        evidence.record("setup", &baseline)?;
        require_native_observations(&baseline)?;
        let seed = json!({"Description":registry::string("Keyrill English PUBLIC-CORRUPTED-SUFFIX"),"IconFile":registry::string(original_path),"IconIndex":registry::dword(0)});
        if profile_metadata(ids)?.iter().any(|value| *value != seed) {
            return Err(
                "disposable corrupted source fixture differs; registration retained".into(),
            );
        }
        let immediate = protected_observation(&mut installation, &manager)?;
        evidence.record("before-update", &immediate)?;
        if protected != immediate {
            return Err(
                "production or source state changed during disposable setup; fixture retained"
                    .into(),
            );
        }
        let forwarding = Rc::new(Forwarding {
            native: manager.clone(),
            ids,
            calls: Cell::new(0),
            result: Cell::new(None),
        });
        let forwarder: ITfInputProcessorProfiles = Forwarder(forwarding.clone()).into();
        let desired = json!({"description":description,"icon":desired_path,"index":0});
        let outcome = profile_write_with(&forwarder, 0, &desired);
        evidence.record("update-result",&json!({"hresult":forwarding.result.get(),"calls":forwarding.calls.get(),"error":outcome.err().map(|error|error.to_string())}))?;
        let after = disposable_observation(ids, &manager)?;
        evidence.record("after-update", &after)?;
        let production = protected_observation(&mut installation, &manager)?;
        evidence.record("after-update-protected", &production)?;
        let expected = json!({"Description":registry::string(description),"IconFile":registry::string(desired["icon"].as_str().unwrap()),"IconIndex":registry::dword(0)});
        if forwarding.calls.get() != 1
            || forwarding.result.get().is_none()
            || profile_metadata(ids)?
                .iter()
                .any(|value| *value != expected)
            || after["profiles"][0]["description"]
                != json!(description.encode_utf16().collect::<Vec<_>>())
            || without_metadata(&baseline, ids) != without_metadata(&after, ids)
            || protected != production
        {
            return Err("native profile qualification mismatch; disposable registration retained without retry or cleanup".into());
        }
        require_native_observations(&after)?;
        fresh.cleanup(
            enable,
            &after,
            &protected,
            &mut installation,
            &manager,
            &mut evidence,
        )?;
    }
    fs::remove_file(original_icon)?;
    fs::remove_file(desired_icon)?;
    fs::remove_dir(icon_directory)?;
    evidence.record("qualification-passed", &json!({"cases":13}))?;
    Ok(())
}

fn require_native_observations(observation: &Value) -> Result<()> {
    for profile in observation["profiles"].as_array().unwrap() {
        if profile["hresults"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hr| hr.as_i64().unwrap() < 0)
        {
            return Err(
                "native profile observation unavailable; disposable registration retained".into(),
            );
        }
    }
    Ok(())
}
