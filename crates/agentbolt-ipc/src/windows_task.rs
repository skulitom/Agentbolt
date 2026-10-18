//! Task Scheduler is machine-wide. Qualify names by the token SID and verify the definition before
//! starting, stopping or removing a task. An unrelated historical name is never adopted or changed.

use agentbolt_core::channel::{Names, env_dir};
use std::path::PathBuf;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{LookupAccountNameW, PSID, SID_NAME_USE};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::System::TaskScheduler::{
    IExecAction, IRegisteredTask, ITaskFolder, ITaskService, TASK_LOGON_INTERACTIVE_TOKEN,
    TASK_RUN_USE_SESSION_ID, TASK_RUNLEVEL_LUA, TaskScheduler,
};
use windows::Win32::System::Variant::VARIANT;
use windows::core::{BSTR, HSTRING, Interface, PCWSTR, PWSTR};

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn name(names: &Names) -> Result<String, String> {
    Ok(format!(
        "{} [{}]",
        names.task,
        crate::windows_util::current_user_sid().map_err(error)?
    ))
}

fn with_root<T>(f: impl FnOnce(ITaskFolder) -> Result<T, String>) -> Result<T, String> {
    // This object is scoped to one synchronous call; COM objects are dropped before uninitializing.
    struct Com(bool);
    impl Drop for Com {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }
    let _com = Com(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok());
    unsafe {
        let service: ITaskService =
            CoCreateInstance(&TaskScheduler, None, CLSCTX_INPROC_SERVER).map_err(error)?;
        let empty = VARIANT::default();
        service
            .Connect(&empty, &empty, &empty, &empty)
            .map_err(error)?;
        f(service.GetFolder(&BSTR::from("\\")).map_err(error)?)
    }
}

fn get(root: &ITaskFolder, name: &str) -> Result<Option<IRegisteredTask>, String> {
    match unsafe { root.GetTask(&BSTR::from(name)) } {
        Ok(task) => Ok(Some(task)),
        Err(e) if e.code() == ERROR_FILE_NOT_FOUND.to_hresult() => Ok(None),
        Err(e) => Err(format!("Reading task '{name}': {e}")),
    }
}

fn account_sid(account: &str) -> Result<String, String> {
    if account.starts_with("S-1-") {
        return Ok(account.to_string());
    }
    unsafe {
        let mut sid_len = 0;
        let mut domain_len = 0;
        let mut kind = SID_NAME_USE::default();
        let user = HSTRING::from(account);
        let _ = LookupAccountNameW(
            PCWSTR::null(),
            &user,
            None,
            &mut sid_len,
            None,
            &mut domain_len,
            &mut kind,
        );
        if sid_len == 0 || sid_len > 4096 || domain_len > 32768 {
            return Err("Task principal could not be resolved".into());
        }
        let mut sid = vec![0u64; (sid_len as usize).div_ceil(8)];
        let mut domain = vec![0u16; domain_len as usize];
        let sid = PSID(sid.as_mut_ptr().cast());
        LookupAccountNameW(
            PCWSTR::null(),
            &user,
            Some(sid),
            &mut sid_len,
            Some(PWSTR(domain.as_mut_ptr())),
            &mut domain_len,
            &mut kind,
        )
        .map_err(error)?;
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(sid, &mut text).map_err(error)?;
        let result = text.to_string().map_err(error);
        LocalFree(Some(HLOCAL(text.0.cast())));
        result
    }
}

fn expected_directory(names: &Names) -> Result<PathBuf, String> {
    Ok(env_dir("LOCALAPPDATA")?
        .join("Programs")
        .join(if names.is_main() {
            "Agentbolt".to_string()
        } else {
            format!("Agentbolt-{}", names.channel)
        }))
}

fn definition_matches(task: &IRegisteredTask, names: &Names) -> Result<bool, String> {
    unsafe {
        let definition = task.Definition().map_err(error)?;
        let principal = definition.Principal().map_err(error)?;
        let mut user = BSTR::new();
        principal.UserId(&mut user).map_err(error)?;
        if account_sid(&user.to_string())?
            != crate::windows_util::current_user_sid().map_err(error)?
        {
            return Ok(false);
        }
        let mut logon = Default::default();
        let mut level = Default::default();
        principal.LogonType(&mut logon).map_err(error)?;
        principal.RunLevel(&mut level).map_err(error)?;
        if logon != TASK_LOGON_INTERACTIVE_TOKEN || level != TASK_RUNLEVEL_LUA {
            return Ok(false);
        }
        let actions = definition.Actions().map_err(error)?;
        let mut count = 0;
        actions.Count(&mut count).map_err(error)?;
        if count != 1 {
            return Ok(false);
        }
        let action: IExecAction = match actions.get_Item(1).map_err(error)?.cast() {
            Ok(exec) => exec,
            Err(_) => return Ok(false),
        };
        let (mut path, mut args, mut cwd) = (BSTR::new(), BSTR::new(), BSTR::new());
        action.Path(&mut path).map_err(error)?;
        action.Arguments(&mut args).map_err(error)?;
        action.WorkingDirectory(&mut cwd).map_err(error)?;
        let path = path.to_string();
        let path = path
            .strip_prefix('"')
            .and_then(|p| p.strip_suffix('"'))
            .unwrap_or(&path);
        let directory = expected_directory(names)?;
        let expected = directory.join("agentbolt-daemon.exe");
        let same_path = path.eq_ignore_ascii_case(&expected.to_string_lossy());
        let same_cwd = cwd
            .to_string()
            .eq_ignore_ascii_case(&directory.to_string_lossy());
        let args = args.to_string();
        Ok(same_path
            && same_cwd
            && (args == format!("--channel {} --supervise", names.channel)
                || args == format!("--channel {}", names.channel)))
    }
}

#[derive(Debug)]
pub struct Registration {
    pub name: String,
    pub xml: String,
}

/// Current scoped task first, followed only by a verified historical task. An inaccessible historical
/// task may belong to another account and is ignored. A collision on our scoped name is a clear error.
pub fn registrations(names: &Names) -> Result<Vec<Registration>, String> {
    let scoped = name(names)?;
    with_root(|root| {
        let mut result = Vec::new();
        for task_name in [&scoped, &names.task] {
            let legacy = task_name == &names.task;
            let task = match get(&root, task_name) {
                Ok(Some(task)) => task,
                Ok(None) => continue,
                Err(_) if legacy => continue,
                Err(e) => return Err(e),
            };
            match definition_matches(&task, names) {
                Ok(true) => result.push(Registration {
                    name: task_name.clone(),
                    xml: unsafe { task.Xml() }.map_err(error)?.to_string(),
                }),
                _ if legacy => continue,
                _ => {
                    return Err(format!(
                        "Task '{task_name}' is not this user's Agentbolt installation; it was left untouched"
                    ));
                }
            }
        }
        Ok(result)
    })
}

impl Registration {
    fn verified(&self, root: &ITaskFolder, names: &Names) -> Result<IRegisteredTask, String> {
        let task =
            get(root, &self.name)?.ok_or_else(|| format!("Task '{}' disappeared", self.name))?;
        if !definition_matches(&task, names)? || unsafe { task.Xml() }.map_err(error)? != self.xml {
            return Err(format!(
                "Task '{}' changed since inspection; left untouched",
                self.name
            ));
        }
        Ok(task)
    }

    pub fn stop(&self, names: &Names) -> Result<(), String> {
        with_root(|root| unsafe { self.verified(&root, names)?.Stop(0) }.map_err(error))
    }

    pub fn ensure_unchanged(&self, names: &Names) -> Result<(), String> {
        with_root(|root| self.verified(&root, names).map(|_| ()))
    }

    pub fn delete(&self, names: &Names) -> Result<(), String> {
        with_root(|root| {
            self.verified(&root, names)?;
            unsafe { root.DeleteTask(&BSTR::from(&self.name), 0) }.map_err(error)
        })
    }
}

pub fn ensure_absent(task_name: &str) -> Result<(), String> {
    with_root(|root| {
        if get(&root, task_name)?.is_some() {
            return Err(format!(
                "Task '{task_name}' appeared during installation; left untouched"
            ));
        }
        Ok(())
    })
}

pub fn run(names: &Names, session: Option<u32>) -> Result<(), String> {
    let task = registrations(names)?
        .into_iter()
        .next()
        .ok_or_else(|| "No verified sign-in task; run `agentbolt install`".to_string())?;
    with_root(|root| unsafe {
        let registered = task.verified(&root, names)?;
        let empty = VARIANT::default();
        if let Some(session) = session {
            registered
                .RunEx(
                    &empty,
                    TASK_RUN_USE_SESSION_ID.0,
                    i32::try_from(session).map_err(error)?,
                    &BSTR::from(crate::windows_util::current_user_sid().map_err(error)?),
                )
                .map_err(error)?;
        } else {
            registered.Run(&empty).map_err(error)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_identity_comes_from_the_token_and_names_remain_channel_specific() {
        let names = Names::for_channel("task-identity-test").unwrap();
        let sid = crate::windows_util::current_user_sid().unwrap();
        assert_eq!(
            name(&names).unwrap(),
            format!("Agentbolt (task-identity-test) [{sid}]")
        );
        assert_ne!(
            name(&names).unwrap(),
            name(&Names::for_channel("main").unwrap()).unwrap()
        );
        assert_eq!(account_sid(&sid).unwrap(), sid);
        let account = format!(
            "{}\\{}",
            std::env::var("USERDOMAIN").unwrap(),
            std::env::var("USERNAME").unwrap()
        );
        assert_eq!(account_sid(&account).unwrap(), sid);
    }
}
