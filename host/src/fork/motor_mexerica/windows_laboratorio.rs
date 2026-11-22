use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::mem::{offset_of, size_of, size_of_val};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::NetworkManagement::WindowsFirewall::NetworkIsolationGetAppContainerConfig;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::Isolation::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::JobObjects::*;
use windows_sys::Win32::System::Memory::{GetProcessHeap, HeapFree};
use windows_sys::Win32::System::Pipes::{CreatePipe, PeekNamedPipe};
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows_sys::Win32::System::SystemServices::{SE_GROUP_ENABLED, SECURITY_MANDATORY_LOW_RID};
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::System::WindowsProgramming::PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;

const REFUSED: &str = "motor_windows_laboratorio_recusado";
const WORKER: &str = "donnabot-motor-catalogo-teste.exe";
const PRIVATE: &str = "somente-hub.txt";
const BROAD_PACKAGES: &str = "pacotes-amplos.txt";
const REQUEST: &[u8] = b"catalogo-sintetico-v1\n";
const RESPONSE: &[u8] = b"catalogo-sintetico-v1:modelo-ficticio\n";
const CHILD_ACL: i32 = 2;
const CHILD_IPC: i32 = 3;
const MEMORY: usize = 256 * 1024 * 1024;
const LIMITS: u32 = JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
const UI: u32 = JOB_OBJECT_UILIMIT_HANDLES
    | JOB_OBJECT_UILIMIT_READCLIPBOARD
    | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
    | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS
    | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
    | JOB_OBJECT_UILIMIT_GLOBALATOMS
    | JOB_OBJECT_UILIMIT_DESKTOP
    | JOB_OBJECT_UILIMIT_EXITWINDOWS;

type Result<T> = std::result::Result<T, &'static str>;

fn require(ok: bool) -> Result<()> {
    if ok { Ok(()) } else { Err(REFUSED) }
}

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain([0]).collect()
}

fn handle(raw: HANDLE) -> Result<OwnedHandle> {
    require(!raw.is_null() && raw != INVALID_HANDLE_VALUE)?;
    // SAFETY: callers only pass newly acquired handles that need CloseHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
}

struct Profile {
    name: Vec<u16>,
    sid: PSID,
}

impl Profile {
    fn new() -> Result<Self> {
        let name = wide(format!("MexericaMotorTeste-{}", uuid::Uuid::new_v4().simple()));
        let mut sid = null_mut();
        // SAFETY: UTF-16 strings are terminated; this profile requests zero capabilities.
        require(
            unsafe { CreateAppContainerProfile(name.as_ptr(), name.as_ptr(), name.as_ptr(), null(), 0, &mut sid) } == 0,
        )?;
        let profile = Self { name, sid };
        require(!profile.sid.is_null())?;
        Ok(profile)
    }

    fn no_loopback(&self) -> Result<()> {
        let mut count = 0;
        let mut entries = null_mut();
        // SAFETY: the API owns allocation, including each SID, on successful return.
        require(unsafe { NetworkIsolationGetAppContainerConfig(&mut count, &mut entries) } == 0)?;
        require(count == 0 || !entries.is_null())?;
        let mut exempt = false;
        // SAFETY: successful API result describes count entries and their heap-allocated SIDs.
        unsafe {
            let heap = GetProcessHeap();
            for i in 0..count as usize {
                let sid = (*entries.add(i)).Sid;
                if sid.is_null() {
                    exempt = true;
                } else {
                    exempt |= EqualSid(self.sid, sid) != 0;
                    HeapFree(heap, 0, sid);
                }
            }
            if !entries.is_null() {
                HeapFree(heap, 0, entries.cast());
            }
        }
        require(!exempt)
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        // SAFETY: this instance owns the profile created for its unique name and SID.
        unsafe {
            DeleteAppContainerProfile(self.name.as_ptr());
            if !self.sid.is_null() {
                FreeSid(self.sid);
            }
        }
    }
}

struct Local(*mut c_void);

impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: all instances own memory allocated by LocalAlloc through a Win32 API.
        unsafe { LocalFree(self.0) };
    }
}

fn sid_text(sid: PSID) -> Result<String> {
    let mut text = null_mut();
    // SAFETY: sid points to a SID owned by an active token buffer or profile.
    require(unsafe { ConvertSidToStringSidW(sid, &mut text) } != 0)?;
    let memory = Local(text.cast());
    // SAFETY: success returns a NUL-terminated string, valid for the lifetime of memory.
    let length = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
    // SAFETY: length counts only the characters before the terminator.
    let text = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) }).map_err(|_| REFUSED)?;
    drop(memory);
    Ok(text)
}

fn descriptor(sddl: &str) -> Result<Local> {
    let mut sd = null_mut();
    // SAFETY: the string is terminated and sd receives a LocalAlloc allocation.
    require(
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(wide(sddl).as_ptr(), 1, &mut sd, null_mut()) }
            != 0,
    )?;
    Ok(Local(sd))
}

fn restrict_path(path: &Path, sddl: &str) -> Result<()> {
    let sd = descriptor(sddl)?;
    let (mut present, mut defaulted, mut acl) = (0, 0, null_mut());
    // SAFETY: the descriptor owns the ACL for the duration of both calls.
    unsafe {
        require(GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted) != 0)?;
        require(present != 0 && !acl.is_null() && defaulted == 0)?;
        require(
            SetNamedSecurityInfoW(
                wide(path).as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null(),
            ) == 0,
        )
    }
}

fn token(process: HANDLE) -> Result<OwnedHandle> {
    let mut raw = null_mut();
    // SAFETY: process is a live process handle or the current-process pseudohandle.
    require(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } != 0)?;
    handle(raw)
}

fn token_data(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS, minimum: usize) -> Result<Vec<usize>> {
    let mut len = 0;
    // SAFETY: this first call only asks for the required size.
    unsafe { GetTokenInformation(token.as_raw_handle(), class, null_mut(), 0, &mut len) };
    require((minimum..=64 * 1024).contains(&(len as usize)))?;
    let mut data = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
    // SAFETY: data is properly aligned, zeroed, and covers len bytes.
    require(
        unsafe { GetTokenInformation(token.as_raw_handle(), class, data.as_mut_ptr().cast(), len, &mut len) } != 0,
    )?;
    require(len as usize >= minimum)?;
    Ok(data)
}

fn token_value(token: &OwnedHandle, class: TOKEN_INFORMATION_CLASS) -> Result<u32> {
    let data = token_data(token, class, size_of::<u32>())?;
    // SAFETY: the queried class returns at least a DWORD at the start of the buffer.
    Ok(unsafe { *data.as_ptr().cast::<u32>() })
}

fn verify_token(child: &OwnedHandle, owner: PSID, expected: PSID) -> Result<()> {
    require(
        token_value(child, TokenIsAppContainer)? == 1
            && token_value(child, TokenIsLessPrivilegedAppContainer)? == 1
            && token_value(child, TokenElevation)? == 0,
    )?;
    let package = token_data(child, TokenAppContainerSid, size_of::<TOKEN_APPCONTAINER_INFORMATION>())?;
    let user = token_data(child, TokenUser, size_of::<TOKEN_USER>())?;
    let integrity = token_data(child, TokenIntegrityLevel, size_of::<TOKEN_MANDATORY_LABEL>())?;
    let groups = token_data(child, TokenGroups, offset_of!(TOKEN_GROUPS, Groups))?;
    let privileges = token_data(child, TokenPrivileges, offset_of!(TOKEN_PRIVILEGES, Privileges))?;
    require(token_value(child, TokenCapabilities)? == 0)?;
    // SAFETY: token_data checked the buffer sizes; the token API fills embedded SID pointers.
    unsafe {
        let package = (*package.as_ptr().cast::<TOKEN_APPCONTAINER_INFORMATION>()).TokenAppContainer;
        let user = (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid;
        let level = (*integrity.as_ptr().cast::<TOKEN_MANDATORY_LABEL>()).Label.Sid;
        require(!package.is_null() && !user.is_null() && !level.is_null())?;
        require(EqualSid(package, expected) != 0 && EqualSid(user, owner) != 0)?;
        let count = u32::from(*GetSidSubAuthorityCount(level));
        require(count > 0 && *GetSidSubAuthority(level, count - 1) == SECURITY_MANDATORY_LOW_RID.cast_unsigned())?;
        let count = *groups.as_ptr().cast::<u32>() as usize;
        require(
            count
                <= (size_of_val(groups.as_slice()) - offset_of!(TOKEN_GROUPS, Groups))
                    / size_of::<SID_AND_ATTRIBUTES>(),
        )?;
        let entries = groups.as_ptr().cast::<u8>().add(offset_of!(TOKEN_GROUPS, Groups)).cast::<SID_AND_ATTRIBUTES>();
        for entry in std::slice::from_raw_parts(entries, count) {
            require(
                entry.Attributes & SE_GROUP_ENABLED.cast_unsigned() == 0
                    || IsWellKnownSid(entry.Sid, WinBuiltinAdministratorsSid) == 0,
            )?;
        }
        let count = *privileges.as_ptr().cast::<u32>() as usize;
        require(
            count
                <= (size_of_val(privileges.as_slice()) - offset_of!(TOKEN_PRIVILEGES, Privileges))
                    / size_of::<LUID_AND_ATTRIBUTES>(),
        )?;
        let entries = privileges
            .as_ptr()
            .cast::<u8>()
            .add(offset_of!(TOKEN_PRIVILEGES, Privileges))
            .cast::<LUID_AND_ATTRIBUTES>();
        let mut traverse = std::mem::zeroed();
        require(LookupPrivilegeValueW(null(), wide("SeChangeNotifyPrivilege").as_ptr(), &mut traverse) != 0)?;
        for entry in std::slice::from_raw_parts(entries, count) {
            require(entry.Luid.HighPart == traverse.HighPart && entry.Luid.LowPart == traverse.LowPart)?;
        }
    }
    Ok(())
}

fn pipe(sd: &Local, child_reads: bool) -> Result<(OwnedHandle, OwnedHandle)> {
    let security = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(|_| REFUSED)?,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let (mut read, mut write) = (null_mut(), null_mut());
    // SAFETY: pointers refer to out handles and a security descriptor valid until return.
    require(unsafe { CreatePipe(&mut read, &mut write, &security, 4096) } != 0)?;
    let (read, write) = (handle(read)?, handle(write)?);
    let (hub, child) = if child_reads { (write, read) } else { (read, write) };
    // SAFETY: only the directional handle for the child becomes inheritable.
    require(unsafe { SetHandleInformation(child.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } != 0)?;
    Ok((hub, child))
}

struct Attributes(LPPROC_THREAD_ATTRIBUTE_LIST, Vec<usize>);

impl Attributes {
    fn new() -> Result<Self> {
        const COUNT: u32 = 5;
        let mut bytes = 0;
        // SAFETY: null requests the required allocation size and does not initialize anything.
        unsafe { InitializeProcThreadAttributeList(null_mut(), COUNT, 0, &mut bytes) };
        require((1..=64 * 1024).contains(&bytes))?;
        let mut allocation = vec![0usize; bytes.div_ceil(size_of::<usize>())];
        let list = allocation.as_mut_ptr().cast();
        // SAFETY: the aligned allocation covers bytes and is kept alive until Delete.
        require(unsafe { InitializeProcThreadAttributeList(list, COUNT, 0, &mut bytes) } != 0)?;
        Ok(Self(list, allocation))
    }

    fn add<T>(&mut self, attribute: u32, value: &T) -> Result<()> {
        // SAFETY: all callers retain the correctly typed storage until CreateProcessW returns.
        require(
            unsafe {
                UpdateProcThreadAttribute(
                    self.0,
                    0,
                    attribute as usize,
                    std::ptr::from_ref(value).cast(),
                    size_of::<T>(),
                    null_mut(),
                    null(),
                )
            } != 0,
        )
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        // SAFETY: this is an initialized list backed by our live allocation.
        unsafe { DeleteProcThreadAttributeList(self.0) };
        let _ = &self.1;
    }
}

fn job() -> Result<OwnedHandle> {
    // SAFETY: no name or attributes, so the returned handle is private and not inheritable.
    let job = handle(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = LIMITS;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    limits.JobMemoryLimit = MEMORY;
    // SAFETY: both calls receive the exact structure for the selected information class.
    unsafe {
        require(
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                u32::try_from(size_of_val(&limits)).map_err(|_| REFUSED)?,
            ) != 0,
        )?;
        require(
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectBasicUIRestrictions,
                std::ptr::from_ref(&UI).cast(),
                4,
            ) != 0,
        )?;
    }
    Ok(job)
}

fn job_info<T: Default>(job: &OwnedHandle, class: JOBOBJECTINFOCLASS) -> Result<T> {
    let mut info = T::default();
    // SAFETY: call sites supply the type that corresponds to class.
    require(
        unsafe {
            QueryInformationJobObject(
                job.as_raw_handle(),
                class,
                std::ptr::from_mut(&mut info).cast(),
                u32::try_from(size_of::<T>()).map_err(|_| REFUSED)?,
                null_mut(),
            )
        } != 0,
    )?;
    Ok(info)
}

fn active(job: &OwnedHandle) -> Result<u32> {
    let info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = job_info(job, JobObjectBasicAccountingInformation)?;
    Ok(info.ActiveProcesses)
}

fn verify_job(job: &OwnedHandle, process: &OwnedHandle) -> Result<()> {
    let mut assigned = 0;
    // SAFETY: both handles are live and assigned points to writable BOOL storage.
    require(
        unsafe { IsProcessInJob(process.as_raw_handle(), job.as_raw_handle(), &mut assigned) } != 0 && assigned != 0,
    )?;
    let limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = job_info(job, JobObjectExtendedLimitInformation)?;
    let ui: u32 = job_info(job, JobObjectBasicUIRestrictions)?;
    require(
        limits.BasicLimitInformation.LimitFlags == LIMITS
            && limits.BasicLimitInformation.ActiveProcessLimit == 1
            && limits.JobMemoryLimit == MEMORY
            && ui == UI
            && active(job)? == 1,
    )
}

fn open_image(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| REFUSED)?;
    let metadata = file.metadata().map_err(|_| REFUSED)?;
    require(metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0)?;
    Ok(file)
}

fn identity_checked(image: &File, actual: &File, expected: [u8; 32]) -> Result<()> {
    require(file_id(image)? == file_id(actual)? && digest(actual)? == expected)
}

fn digest(mut file: &File) -> Result<[u8; 32]> {
    file.rewind().map_err(|_| REFUSED)?;
    let mut digest = Sha256::new();
    let mut bytes = [0; 8192];
    loop {
        let len = file.read(&mut bytes).map_err(|_| REFUSED)?;
        if len == 0 {
            break;
        }
        digest.update(&bytes[..len]);
    }
    Ok(digest.finalize().into())
}

fn file_id(file: &File) -> Result<(u32, u32, u32)> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file handle is valid and info has the Win32 layout.
    require(unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } != 0)?;
    Ok((info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
}

fn verify_image(process: &OwnedHandle, locked: &File, expected: [u8; 32]) -> Result<()> {
    let mut name = vec![0u16; 32_768];
    let mut len = u32::try_from(name.len()).map_err(|_| REFUSED)?;
    // SAFETY: name covers len WCHARs and the process is still suspended.
    require(unsafe { QueryFullProcessImageNameW(process.as_raw_handle(), 0, name.as_mut_ptr(), &mut len) } != 0)?;
    let path = PathBuf::from(OsString::from_wide(&name[..len as usize]));
    let actual = open_image(&path)?;
    identity_checked(locked, &actual, expected)
}

#[derive(Clone, Copy, Debug, Default)]
enum Stage {
    #[default]
    Preparo,
    Perfil,
    Job,
    Atributos,
    AtributoLpac,
    CriacaoSuspensa,
    TokenLpac,
    Imagem,
    Loopback,
    Handles,
    RecusaEsperada,
    Retomada,
    Ipc,
    Filho,
    Termino,
}

#[derive(Default)]
struct Lab {
    unconfirmed: bool,
    stage: Stage,
    child_stage: Option<i32>,
    create_error: Option<u32>,
}

impl Lab {
    fn report_refusal(&self) {
        eprintln!("LAB_STAGE={:?}", self.stage);
        if let Some(code) = self.create_error {
            eprintln!("LAB_WIN32={code}");
        }
        if let Some(stage) = self.child_stage {
            eprintln!("CHILD_STAGE={stage}");
        }
    }

    fn catalog(&mut self, before_resume: impl FnOnce() -> Result<()>) -> Result<&'static [u8]> {
        self.stage = Stage::Preparo;
        self.child_stage = None;
        self.create_error = None;
        require(!self.unconfirmed)?;
        // SAFETY: GetCurrentProcess returns the current process pseudohandle.
        let parent = token(unsafe { GetCurrentProcess() })?;
        require(token_value(&parent, TokenElevation)? == 0 && token_value(&parent, TokenIsAppContainer)? == 0)?;
        let owner = token_data(&parent, TokenUser, size_of::<TOKEN_USER>())?;
        // SAFETY: owner is a live aligned buffer filled with TOKEN_USER.
        let owner_sid = unsafe { (*owner.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let owner_text = sid_text(owner_sid)?;
        self.stage = Stage::Perfil;
        let profile = Profile::new()?;
        self.stage = Stage::Loopback;
        profile.no_loopback()?;
        self.stage = Stage::Preparo;
        let package = sid_text(profile.sid)?;
        let root = tempfile::tempdir().map_err(|_| REFUSED)?;
        require(
            fs::symlink_metadata(root.path()).map_err(|_| REFUSED)?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT
                == 0,
        )?;
        restrict_path(root.path(), &format!("D:P(A;;FA;;;{owner_text})(A;;FRFX;;;{package})"))?;
        let image = root.path().join(WORKER);
        let mut original = open_image(&std::env::current_exe().map_err(|_| REFUSED)?)?;
        let expected = digest(&original)?;
        original.rewind().map_err(|_| REFUSED)?;
        io::copy(&mut original, &mut File::create_new(&image).map_err(|_| REFUSED)?).map_err(|_| REFUSED)?;
        restrict_path(&image, &format!("D:P(A;;FA;;;{owner_text})(A;;FRFX;;;{package})"))?;
        let locked = open_image(&image)?;
        require(digest(&locked)? == expected)?;
        let private = root.path().join(PRIVATE);
        File::create_new(&private).and_then(|mut f| f.write_all(b"sentinela-ficticia")).map_err(|_| REFUSED)?;
        restrict_path(&private, &format!("D:P(A;;FA;;;{owner_text})"))?;
        let broad = root.path().join(BROAD_PACKAGES);
        File::create_new(&broad).and_then(|mut f| f.write_all(b"sentinela-pacotes")).map_err(|_| REFUSED)?;
        restrict_path(&broad, &format!("D:P(A;;FA;;;{owner_text})(A;;FR;;;S-1-15-2-1)"))?;
        require(fs::read(&broad).map_err(|_| REFUSED)? == b"sentinela-pacotes")?;

        let sd = descriptor(&format!("D:P(A;;GA;;;{owner_text})"))?;
        let (input, child_in) = pipe(&sd, true)?;
        let (quiet, child_out) = pipe(&sd, false)?;
        let (output, child_reply) = pipe(&sd, false)?;
        let (marker, not_allowlisted) = pipe(&sd, false)?;
        self.stage = Stage::Job;
        let job = job()?;
        let jobs = [job.as_raw_handle()];
        let inherited = [child_in.as_raw_handle(), child_out.as_raw_handle(), child_reply.as_raw_handle()];
        let capabilities = SECURITY_CAPABILITIES { AppContainerSid: profile.sid, ..Default::default() };
        let no_children = 1u32;
        let package_policy = PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
        self.stage = Stage::Atributos;
        let mut attrs = Attributes::new()?;
        attrs.add(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
        attrs.add(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherited)?;
        attrs.add(PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES, &capabilities)?;
        attrs.add(PROC_THREAD_ATTRIBUTE_CHILD_PROCESS_POLICY, &no_children)?;
        self.stage = Stage::AtributoLpac;
        attrs.add(PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY, &package_policy)?;
        self.stage = Stage::CriacaoSuspensa;
        let mut start = STARTUPINFOEXW::default();
        start.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(|_| REFUSED)?;
        start.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        [start.StartupInfo.hStdInput, start.StartupInfo.hStdOutput, start.StartupInfo.hStdError] = inherited;
        start.lpAttributeList = attrs.0;
        let (_, tests) = module_path!().split_once("::").ok_or(REFUSED)?;
        let mut command = wide(format!("{WORKER} --ignored --exact {tests}::catalog_child --quiet"));
        let mut system_root = vec![0u16; 32_768];
        let capacity = u32::try_from(system_root.len()).map_err(|_| REFUSED)?;
        // SAFETY: system_root has room for capacity UTF-16 units.
        let length = unsafe { GetSystemWindowsDirectoryW(system_root.as_mut_ptr(), capacity) } as usize;
        require(length > 0 && length < system_root.len() && system_root[length] == 0)?;
        let system_root = &system_root[..length];
        require(!system_root.contains(&0))?;
        let root_path = PathBuf::from(OsString::from_wide(system_root));
        require(root_path.is_absolute() && root_path.is_dir())?;
        let mut environment: Vec<u16> = "SystemRoot=".encode_utf16().collect();
        environment.extend_from_slice(system_root);
        environment.extend([0, 0]);
        let mut process = PROCESS_INFORMATION::default();
        // SAFETY: terminated paths/command, a writable command line, an explicit UTF-16 environment,
        // and live attributes/handles. The process enters the Job atomically and starts suspended.
        let image_path = wide(&image);
        let current_dir = wide(root.path());
        let created = unsafe {
            CreateProcessW(
                image_path.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment.as_ptr().cast(),
                current_dir.as_ptr(),
                &start.StartupInfo,
                &mut process,
            )
        };
        if created == 0 {
            // SAFETY: reads this thread's last error immediately after CreateProcessW fails.
            let code = unsafe { GetLastError() };
            self.create_error = (1..=99_999).contains(&code).then_some(code);
            return Err(REFUSED);
        }
        self.unconfirmed = true;
        let process_handle = handle(process.hProcess);
        let thread = handle(process.hThread);
        drop((attrs, child_in, child_out, child_reply, not_allowlisted));
        let result = (|| {
            let (process, thread) =
                (process_handle.as_ref().map_err(|_| REFUSED)?, thread.as_ref().map_err(|_| REFUSED)?);
            self.stage = Stage::Job;
            verify_job(&job, process)?;
            self.stage = Stage::TokenLpac;
            verify_token(&token(process.as_raw_handle())?, owner_sid, profile.sid)?;
            self.stage = Stage::Imagem;
            verify_image(process, &locked, expected)?;
            self.stage = Stage::Loopback;
            profile.no_loopback()?;
            self.stage = Stage::Handles;
            // SAFETY: marker is an open read handle. Its inheritable writer was not allowlisted.
            require(unsafe {
                PeekNamedPipe(marker.as_raw_handle(), null_mut(), 0, null_mut(), null_mut(), null_mut()) == 0
                    && GetLastError() == ERROR_BROKEN_PIPE
            })?;
            self.stage = Stage::RecusaEsperada;
            before_resume()?;
            self.stage = Stage::Retomada;
            // SAFETY: live thread handle of the verified, still-suspended worker.
            require(unsafe { ResumeThread(thread.as_raw_handle()) } == 1)?;
            self.stage = Stage::Ipc;
            File::from(input).write_all(REQUEST).map_err(|_| REFUSED)?;
            self.stage = Stage::Filho;
            // SAFETY: bounded wait on the verified worker; no blocking read before exit.
            require(unsafe { WaitForSingleObject(process.as_raw_handle(), 5_000) } == WAIT_OBJECT_0)?;
            let mut exit = 1;
            // SAFETY: exit is writable and the process handle remains live after exit.
            require(unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit) } != 0)?;
            self.child_stage = match i32::try_from(exit) {
                Ok(stage @ (CHILD_ACL | CHILD_IPC)) => Some(stage),
                _ => None,
            };
            require(exit == 0)?;
            self.stage = Stage::Termino;
            require(active(&job)? == 0)?;
            self.stage = Stage::Ipc;
            let mut reply = Vec::new();
            File::from(output).take((RESPONSE.len() + 1) as u64).read_to_end(&mut reply).map_err(|_| REFUSED)?;
            require(reply == RESPONSE)?;
            Ok(RESPONSE)
        })();
        drop(quiet);
        if result.is_err() {
            // SAFETY: this is the exclusively owned Job, never the current process's Job.
            unsafe { TerminateJobObject(job.as_raw_handle(), 1) };
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let stage = self.stage;
        self.stage = Stage::Termino;
        loop {
            if active(&job)? == 0 {
                self.unconfirmed = false;
                break;
            }
            require(Instant::now() < deadline)?;
            std::thread::sleep(Duration::from_millis(10));
        }
        self.stage = stage;
        result
    }
}

#[test]
#[ignore = "worker privado: iniciado somente pelo teste de contenção"]
fn catalog_child() {
    // SAFETY: GetCurrentProcess returns the current process pseudohandle.
    let worker = token(unsafe { GetCurrentProcess() }).and_then(|t| token_value(&t, TokenIsLessPrivilegedAppContainer));
    if worker != Ok(1) {
        return;
    }
    let mut failure = CHILD_ACL;
    let result = (|| -> Result<()> {
        require(fs::read(PRIVATE).is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied))?;
        require(fs::read(BROAD_PACKAGES).is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied))?;
        failure = CHILD_IPC;
        // SAFETY: these are the allowlisted standard handles owned by this process; each is taken once.
        unsafe {
            use windows_sys::Win32::System::Console::{
                GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
            };
            let input = handle(GetStdHandle(STD_INPUT_HANDLE))?;
            let output = handle(GetStdHandle(STD_ERROR_HANDLE))?;
            for raw in [input.as_raw_handle(), output.as_raw_handle(), GetStdHandle(STD_OUTPUT_HANDLE)] {
                require(SetHandleInformation(raw, HANDLE_FLAG_INHERIT, 0) != 0)?;
            }
            let mut request = [0; REQUEST.len()];
            File::from(input).read_exact(&mut request).map_err(|_| REFUSED)?;
            require(request == REQUEST)?;
            File::from(output).write_all(RESPONSE).map_err(|_| REFUSED)?;
        }
        Ok(())
    })();
    std::process::exit(if result.is_err() { failure } else { 0 });
}

#[test]
fn windows_nativo_catalogo_e_recusa_antes_de_retomar() {
    let mut lab = Lab::default();
    let mut verified = false;
    let refusal = lab.catalog(|| {
        verified = true;
        Err(REFUSED)
    });
    if refusal != Err(REFUSED) || !verified || lab.unconfirmed {
        lab.report_refusal();
    }
    assert_eq!(refusal, Err(REFUSED));
    assert!(verified, "o negativo precisa chegar ao filho suspenso e verificado");
    assert!(!lab.unconfirmed);
    let catalog = lab.catalog(|| Ok(()));
    if catalog != Ok(RESPONSE) || lab.unconfirmed {
        lab.report_refusal();
    }
    assert_eq!(catalog, Ok(RESPONSE));
    assert!(!lab.unconfirmed);
}

#[test]
fn simulacao_nao_readmite_com_termino_indeterminado() {
    let mut lab = Lab { unconfirmed: true, ..Lab::default() };
    assert_eq!(lab.catalog(|| panic!("não pode admitir processo")), Err(REFUSED));
    assert!(lab.unconfirmed);
}

#[test]
fn windows_nativo_identidade_da_imagem_recusa_copia_ou_hash_errado() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("esperado");
    let copy = root.path().join("outra-identidade");
    fs::write(&file, b"imagem-ficticia").unwrap();
    fs::copy(&file, &copy).unwrap();
    let file = open_image(&file).unwrap();
    let copy = open_image(&copy).unwrap();
    let expected = digest(&file).unwrap();
    assert_eq!(identity_checked(&file, &file, expected), Ok(()));
    assert_eq!(identity_checked(&file, &copy, expected), Err(REFUSED));
    assert_eq!(identity_checked(&file, &file, [0; 32]), Err(REFUSED));
}
