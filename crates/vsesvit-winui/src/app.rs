//! The XAML `Application` subclass and the process exit code.

use std::cell::{Cell, RefCell};
use std::process::ExitCode;

use windows_core::{Array, HSTRING, Interface, Ref, Result, implement};

use crate::bindings::*;
use crate::browser::{self, Launch};
use crate::exec;

thread_local! {
    static EXIT_CODE: Cell<u8> = const { Cell::new(0) };
    static EXITING: Cell<bool> = const { Cell::new(false) };
    static APPLICATION: RefCell<Option<Application>> = const { RefCell::new(None) };
}

/// Runs XAML on the calling thread until the last window closes or `exit` is called.
pub(crate) fn run(launch: Launch) -> ExitCode {
    let launch = RefCell::new(Some(launch));
    let started = Application::Start(&ApplicationInitializationCallback::new(move |_| {
        let app = App {
            provider: RefCell::new(None),
            launch: RefCell::new(launch.take()),
        };
        match Application::compose(app) {
            Ok(app) => APPLICATION.with_borrow_mut(|a| *a = Some(app)),
            Err(e) => log::error!("creating the XAML application: {e}"),
        }
    }));
    // XAML has shut down; releasing its objects now can fault, and the process is ending.
    std::mem::forget(APPLICATION.with_borrow_mut(Option::take));
    if let Err(e) = started {
        log::error!("XAML application: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::from(EXIT_CODE.get())
}

/// Ends the XAML message loop; `run` then returns `code`. Everything that holds XAML or
/// WebView2 objects is released first, while the framework is still alive. The first call
/// decides the code: windows closing during the shutdown must not turn a failure into success.
pub(crate) fn exit(code: u8) {
    if EXITING.replace(true) {
        return;
    }
    EXIT_CODE.set(code);
    browser::shutdown();
    exec::shutdown();
    if let Err(e) = Application::Current().and_then(|app| app.Exit()) {
        log::error!("exit: {e}");
        std::process::exit(i32::from(code));
    }
}

#[implement(IApplicationOverrides, IXamlMetadataProvider)]
struct App {
    provider: RefCell<Option<XamlControlsXamlMetaDataProvider>>,
    launch: RefCell<Option<Launch>>,
}

impl App_Impl {
    fn provider(&self) -> Result<XamlControlsXamlMetaDataProvider> {
        if let Some(provider) = self.provider.borrow().as_ref() {
            return Ok(provider.clone());
        }
        let provider = XamlControlsXamlMetaDataProvider::new()?;
        *self.provider.borrow_mut() = Some(provider.clone());
        Ok(provider)
    }
}

impl IApplicationOverrides_Impl for App_Impl {
    fn OnLaunched(&self, _args: Ref<LaunchActivatedEventArgs>) -> Result<()> {
        // An error returned from here kills the process with 0xC000027B, so report and exit.
        if let Err(e) = self.launch() {
            log::error!("launch failed: {e}");
            exit(1);
        }
        Ok(())
    }
}

impl App_Impl {
    fn launch(&self) -> Result<()> {
        // WinUI controls need their resources merged after Start (microsoft-ui-xaml #7606).
        let controls: ResourceDictionary = XamlControlsResources::new()?.cast()?;
        Application::Current()?
            .Resources()?
            .MergedDictionaries()?
            .Append(&controls)?;
        exec::init()?;
        let launch = self
            .launch
            .borrow_mut()
            .take()
            .ok_or_else(windows_core::Error::empty)?;
        browser::launch(launch);
        Ok(())
    }
}

impl IXamlMetadataProvider_Impl for App_Impl {
    fn GetXamlType(&self, r#type: &TypeName) -> Result<IXamlType> {
        self.provider()?.GetXamlType(r#type)
    }

    fn GetXamlTypeByFullName(&self, name: &HSTRING) -> Result<IXamlType> {
        self.provider()?
            .GetXamlTypeByFullName(&name.to_string_lossy())
    }

    fn GetXmlnsDefinitions(&self) -> Result<Array<XmlnsDefinition>> {
        self.provider()?.GetXmlnsDefinitions()
    }
}
