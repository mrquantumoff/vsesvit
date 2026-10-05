windows_core::link!("api-ms-win-appmodel-runtime-l1-1-5.dll" "system" fn AddPackageDependency(packagedependencyid : windows_core::PCWSTR, rank : i32, options : AddPackageDependencyOptions, packagedependencycontext : *mut PACKAGEDEPENDENCY_CONTEXT, packagefullname : *mut windows_core::PWSTR) -> windows_core::HRESULT);
windows_core::link!("user32.dll" "system" fn AllowSetForegroundWindow(dwprocessid : u32) -> windows_core::BOOL);
windows_core::link!("kernel32.dll" "system" fn AttachConsole(dwprocessid : u32) -> windows_core::BOOL);
windows_core::link!("crypt32.dll" "system" fn CertAddEncodedCertificateToStore(hcertstore : HCERTSTORE, dwcertencodingtype : u32, pbcertencoded : *const u8, cbcertencoded : u32, dwadddisposition : u32, ppcertcontext : *mut PCCERT_CONTEXT) -> windows_core::BOOL);
windows_core::link!("crypt32.dll" "system" fn CertCloseStore(hcertstore : HCERTSTORE, dwflags : u32) -> windows_core::BOOL);
windows_core::link!("crypt32.dll" "system" fn CertFreeCertificateContext(pcertcontext : *const CERT_CONTEXT) -> windows_core::BOOL);
windows_core::link!("crypt32.dll" "system" fn CertOpenStore(lpszstoreprovider : windows_core::PCSTR, dwencodingtype : u32, hcryptprov : HCRYPTPROV_LEGACY, dwflags : u32, pvpara : *const core::ffi::c_void) -> HCERTSTORE);
windows_core::link!("ole32.dll" "system" fn CoInitializeEx(pvreserved : *const core::ffi::c_void, dwcoinit : u32) -> windows_core::HRESULT);
windows_core::link!("ole32.dll" "system" fn CoTaskMemFree(pv : *mut core::ffi::c_void));
windows_core::link!("shell32.dll" "system" fn CommandLineToArgvW(lpcmdline : windows_core::PCWSTR, pnumargs : *mut i32) -> *mut windows_core::PWSTR);
windows_core::link!("d3d11.dll" "system" fn CreateDirect3D11DeviceFromDXGIDevice(dxgidevice : *mut core::ffi::c_void, graphicsdevice : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
windows_core::link!("cryptui.dll" "system" fn CryptUIDlgViewContext(dwcontexttype : u32, pvcontext : *const core::ffi::c_void, hwnd : HWND, pwsztitle : windows_core::PCWSTR, dwflags : u32, pvreserved : *const core::ffi::c_void) -> windows_core::BOOL);
windows_core::link!("d3d11.dll" "system" fn D3D11CreateDevice(padapter : *mut core::ffi::c_void, drivertype : D3D_DRIVER_TYPE, software : HMODULE, flags : u32, pfeaturelevels : *const D3D_FEATURE_LEVEL, featurelevels : u32, sdkversion : u32, ppdevice : *mut *mut core::ffi::c_void, pfeaturelevel : *mut D3D_FEATURE_LEVEL, ppimmediatecontext : *mut *mut core::ffi::c_void) -> windows_core::HRESULT);
windows_core::link!("dwmapi.dll" "system" fn DwmGetWindowAttribute(hwnd : HWND, dwattribute : u32, pvattribute : *mut core::ffi::c_void, cbattribute : u32) -> windows_core::HRESULT);
windows_core::link!("user32.dll" "system" fn EnumWindows(lpenumfunc : WNDENUMPROC, lparam : LPARAM) -> windows_core::BOOL);
windows_core::link!("user32.dll" "system" fn GetAncestor(hwnd : HWND, gaflags : u32) -> HWND);
windows_core::link!("user32.dll" "system" fn GetAsyncKeyState(vkey : i32) -> i16);
windows_core::link!("user32.dll" "system" fn GetDpiForWindow(hwnd : HWND) -> u32);
windows_core::link!("user32.dll" "system" fn GetForegroundWindow() -> HWND);
windows_core::link!("user32.dll" "system" fn GetKeyState(nvirtkey : i32) -> i16);
windows_core::link!("kernel32.dll" "system" fn GetModuleHandleW(lpmodulename : windows_core::PCWSTR) -> HMODULE);
windows_core::link!("kernel32.dll" "system" fn GetProcAddress(hmodule : HMODULE, lpprocname : windows_core::PCSTR) -> FARPROC);
windows_core::link!("kernel32.dll" "system" fn GetProcessHeap() -> HANDLE);
windows_core::link!("user32.dll" "system" fn GetSystemMetrics(nindex : i32) -> i32);
windows_core::link!("user32.dll" "system" fn GetWindowTextW(hwnd : HWND, lpstring : windows_core::PWSTR, nmaxcount : i32) -> i32);
windows_core::link!("user32.dll" "system" fn GetWindowThreadProcessId(hwnd : HWND, lpdwprocessid : *mut u32) -> u32);
windows_core::link!("kernel32.dll" "system" fn HeapFree(hheap : HANDLE, dwflags : u32, lpmem : *mut core::ffi::c_void) -> windows_core::BOOL);
windows_core::link!("user32.dll" "system" fn IsWindowVisible(hwnd : HWND) -> windows_core::BOOL);
windows_core::link!("user32.dll" "system" fn LoadImageW(hinst : HINSTANCE, name : windows_core::PCWSTR, r#type : u32, cx : i32, cy : i32, fuload : u32) -> HANDLE);
windows_core::link!("kernel32.dll" "system" fn LoadLibraryExW(lplibfilename : windows_core::PCWSTR, hfile : HANDLE, dwflags : u32) -> HMODULE);
windows_core::link!("kernel32.dll" "system" fn LocalFree(hmem : HLOCAL) -> HLOCAL);
windows_core::link!("user32.dll" "system" fn MessageBoxW(hwnd : HWND, lptext : windows_core::PCWSTR, lpcaption : windows_core::PCWSTR, utype : u32) -> i32);
windows_core::link!("user32.dll" "system" fn PostMessageW(hwnd : HWND, msg : u32, wparam : WPARAM, lparam : LPARAM) -> windows_core::BOOL);
windows_core::link!("advapi32.dll" "system" fn RegGetValueW(hkey : HKEY, lpsubkey : windows_core::PCWSTR, lpvalue : windows_core::PCWSTR, dwflags : u32, pdwtype : *mut u32, pvdata : *mut core::ffi::c_void, pcbdata : *mut u32) -> LSTATUS);
windows_core::link!("shell32.dll" "system" fn SHGetKnownFolderPath(rfid : *const KNOWNFOLDERID, dwflags : u32, htoken : HANDLE, ppszpath : *mut windows_core::PWSTR) -> windows_core::HRESULT);
windows_core::link!("user32.dll" "system" fn SendInput(cinputs : u32, pinputs : *const INPUT, cbsize : i32) -> u32);
windows_core::link!("user32.dll" "system" fn SendMessageW(hwnd : HWND, msg : u32, wparam : WPARAM, lparam : LPARAM) -> LRESULT);
windows_core::link!("user32.dll" "system" fn SetProcessDpiAwarenessContext(value : DPI_AWARENESS_CONTEXT) -> windows_core::BOOL);
windows_core::link!("shell32.dll" "system" fn ShellExecuteW(hwnd : HWND, lpoperation : windows_core::PCWSTR, lpfile : windows_core::PCWSTR, lpparameters : windows_core::PCWSTR, lpdirectory : windows_core::PCWSTR, nshowcmd : i32) -> HINSTANCE);
windows_core::link!("user32.dll" "system" fn SystemParametersInfoW(uiaction : u32, uiparam : u32, pvparam : *mut core::ffi::c_void, fwinini : u32) -> windows_core::BOOL);
windows_core::link!("api-ms-win-appmodel-runtime-l1-1-5.dll" "system" fn TryCreatePackageDependency(user : PSID, packagefamilyname : windows_core::PCWSTR, minversion : PACKAGE_VERSION, packagedependencyprocessorarchitectures : PackageDependencyProcessorArchitectures, lifetimekind : PackageDependencyLifetimeKind, lifetimeartifact : windows_core::PCWSTR, options : CreatePackageDependencyOptions, packagedependencyid : *mut windows_core::PWSTR) -> windows_core::HRESULT);
pub const ATTACH_PARENT_PROCESS: u32 = 4294967295;
pub type AddPackageDependencyOptions = u32;
pub const AddPackageDependencyOptions_None: AddPackageDependencyOptions = 0;
pub const AddPackageDependencyOptions_PrependIfRankCollision: AddPackageDependencyOptions = 1;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppActivationArguments(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppActivationArguments,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppActivationArguments {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppActivationArguments>();
}
unsafe impl windows_core::Interface for AppActivationArguments {
    type Vtable = <IAppActivationArguments as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppActivationArguments as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppActivationArguments {
    type Target = IAppActivationArguments;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppActivationArguments {
    const NAME: &'static str = "Microsoft.Windows.AppLifecycle.AppActivationArguments";
}
unsafe impl Send for AppActivationArguments {}
unsafe impl Sync for AppActivationArguments {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppInstance(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppInstance,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl AppInstance {
    pub fn GetCurrent() -> windows_core::Result<Self> {
        Self::IAppInstanceStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetCurrent)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn FindOrRegisterForKey(key: &str) -> windows_core::Result<Self> {
        Self::IAppInstanceStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).FindOrRegisterForKey)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(key)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IAppInstanceStatics<R, F: FnOnce(&IAppInstanceStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<AppInstance, IAppInstanceStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for AppInstance {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppInstance>();
}
unsafe impl windows_core::Interface for AppInstance {
    type Vtable = <IAppInstance as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppInstance as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppInstance {
    type Target = IAppInstance;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppInstance {
    const NAME: &'static str = "Microsoft.Windows.AppLifecycle.AppInstance";
}
unsafe impl Send for AppInstance {}
unsafe impl Sync for AppInstance {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindow(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindow,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindow>();
}
unsafe impl windows_core::Interface for AppWindow {
    type Vtable = <IAppWindow as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindow as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindow {
    type Target = IAppWindow;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindow {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindow";
}
unsafe impl Send for AppWindow {}
unsafe impl Sync for AppWindow {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindowPresenter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindowPresenter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindowPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindowPresenter>();
}
unsafe impl windows_core::Interface for AppWindowPresenter {
    type Vtable = <IAppWindowPresenter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindowPresenter as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindowPresenter {
    type Target = IAppWindowPresenter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindowPresenter {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindowPresenter";
}
unsafe impl Send for AppWindowPresenter {}
unsafe impl Sync for AppWindowPresenter {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AppWindowPresenterKind(pub i32);
impl AppWindowPresenterKind {
    pub const Default: Self = Self(0);
    pub const CompactOverlay: Self = Self(1);
    pub const FullScreen: Self = Self(2);
    pub const Overlapped: Self = Self(3);
}
impl windows_core::imp::TypeKind for AppWindowPresenterKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for AppWindowPresenterKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Windowing.AppWindowPresenterKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppWindowTitleBar(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AppWindowTitleBar,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for AppWindowTitleBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAppWindowTitleBar>();
}
unsafe impl windows_core::Interface for AppWindowTitleBar {
    type Vtable = <IAppWindowTitleBar as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAppWindowTitleBar as windows_core::Interface>::IID;
}
impl core::ops::Deref for AppWindowTitleBar {
    type Target = IAppWindowTitleBar;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AppWindowTitleBar {
    const NAME: &'static str = "Microsoft.UI.Windowing.AppWindowTitleBar";
}
unsafe impl Send for AppWindowTitleBar {}
unsafe impl Sync for AppWindowTitleBar {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Application(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Application,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl Application {
    pub fn compose<T>(compose: T) -> windows_core::Result<Self>
    where
        T: windows_core::Compose,
    {
        Self::IApplicationFactory(|this| unsafe {
            let (derived__, base__) = windows_core::Compose::compose(compose);
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&derived__),
                base__ as *mut _ as _,
                &mut result__,
            )
            .ok()?;
            let _ = &derived__;
            windows_core::imp::Type::from_abi(result__)
        })
    }
    pub fn Current() -> windows_core::Result<Self> {
        Self::IApplicationStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Current)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn Start<P0>(callback: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ApplicationInitializationCallback>,
    {
        Self::IApplicationStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).Start)(
                windows_core::Interface::as_raw(this),
                callback.param().abi(),
            )
            .ok()
        })
    }
    fn IApplicationFactory<R, F: FnOnce(&IApplicationFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Application, IApplicationFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IApplicationStatics<R, F: FnOnce(&IApplicationStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Application, IApplicationStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Application {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IApplication>();
}
unsafe impl windows_core::Interface for Application {
    type Vtable = <IApplication as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IApplication as windows_core::Interface>::IID;
}
impl core::ops::Deref for Application {
    type Target = IApplication;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Application {
    const NAME: &'static str = "Microsoft.UI.Xaml.Application";
}
unsafe impl Send for Application {}
unsafe impl Sync for Application {}
windows_core::imp::define_interface!(
    ApplicationInitializationCallback,
    ApplicationInitializationCallback_Vtbl,
    0xd8eef1c9_1234_56f1_9963_45dd9c80a661
);
impl windows_core::RuntimeType for ApplicationInitializationCallback {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ApplicationInitializationCallback {
    pub fn new<F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static>(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &ApplicationInitializationCallbackBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
}
#[repr(C)]
pub struct ApplicationInitializationCallback_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        p: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct ApplicationInitializationCallbackBox<
    F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<F: Fn(windows_core::Ref<ApplicationInitializationCallbackParams>) + 'static>
    ApplicationInitializationCallbackBox<F>
{
    const VTABLE: ApplicationInitializationCallback_Vtbl = ApplicationInitializationCallback_Vtbl {
        base__:
            windows_core::IUnknown_Vtbl {
                QueryInterface: windows_core::imp::DelegateBox::<
                    ApplicationInitializationCallback,
                    F,
                >::QueryInterface,
                AddRef:
                    windows_core::imp::DelegateBox::<ApplicationInitializationCallback, F>::AddRef,
                Release:
                    windows_core::imp::DelegateBox::<ApplicationInitializationCallback, F>::Release,
            },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        p: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<ApplicationInitializationCallback, F>);
            (this.invoke)(core::mem::transmute_copy(&p));
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationInitializationCallbackParams(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ApplicationInitializationCallbackParams,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for ApplicationInitializationCallbackParams {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        IApplicationInitializationCallbackParams,
    >();
}
unsafe impl windows_core::Interface for ApplicationInitializationCallbackParams {
    type Vtable = <IApplicationInitializationCallbackParams as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IApplicationInitializationCallbackParams as windows_core::Interface>::IID;
}
impl core::ops::Deref for ApplicationInitializationCallbackParams {
    type Target = IApplicationInitializationCallbackParams;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ApplicationInitializationCallbackParams {
    const NAME: &'static str = "Microsoft.UI.Xaml.ApplicationInitializationCallbackParams";
}
unsafe impl Send for ApplicationInitializationCallbackParams {}
unsafe impl Sync for ApplicationInitializationCallbackParams {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoSuggestBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AutoSuggestBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    AutoSuggestBox,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for AutoSuggestBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBox>();
}
unsafe impl windows_core::Interface for AutoSuggestBox {
    type Vtable = <IAutoSuggestBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAutoSuggestBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for AutoSuggestBox {
    type Target = IAutoSuggestBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AutoSuggestBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBox";
}
unsafe impl Send for AutoSuggestBox {}
unsafe impl Sync for AutoSuggestBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoSuggestBoxQuerySubmittedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AutoSuggestBoxQuerySubmittedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(AutoSuggestBoxQuerySubmittedEventArgs, DependencyObject);
impl windows_core::RuntimeType for AutoSuggestBoxQuerySubmittedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBoxQuerySubmittedEventArgs>();
}
unsafe impl windows_core::Interface for AutoSuggestBoxQuerySubmittedEventArgs {
    type Vtable = <IAutoSuggestBoxQuerySubmittedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IAutoSuggestBoxQuerySubmittedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for AutoSuggestBoxQuerySubmittedEventArgs {
    type Target = IAutoSuggestBoxQuerySubmittedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AutoSuggestBoxQuerySubmittedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBoxQuerySubmittedEventArgs";
}
unsafe impl Send for AutoSuggestBoxQuerySubmittedEventArgs {}
unsafe impl Sync for AutoSuggestBoxQuerySubmittedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutoSuggestBoxTextChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AutoSuggestBoxTextChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(AutoSuggestBoxTextChangedEventArgs, DependencyObject);
impl windows_core::RuntimeType for AutoSuggestBoxTextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutoSuggestBoxTextChangedEventArgs>();
}
unsafe impl windows_core::Interface for AutoSuggestBoxTextChangedEventArgs {
    type Vtable = <IAutoSuggestBoxTextChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IAutoSuggestBoxTextChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for AutoSuggestBoxTextChangedEventArgs {
    type Target = IAutoSuggestBoxTextChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AutoSuggestBoxTextChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.AutoSuggestBoxTextChangedEventArgs";
}
unsafe impl Send for AutoSuggestBoxTextChangedEventArgs {}
unsafe impl Sync for AutoSuggestBoxTextChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AutoSuggestionBoxTextChangeReason(pub i32);
impl AutoSuggestionBoxTextChangeReason {
    pub const UserInput: Self = Self(0);
    pub const ProgrammaticChange: Self = Self(1);
    pub const SuggestionChosen: Self = Self(2);
}
impl windows_core::imp::TypeKind for AutoSuggestionBoxTextChangeReason {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for AutoSuggestionBoxTextChangeReason {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.AutoSuggestionBoxTextChangeReason;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationPeer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AutomationPeer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(AutomationPeer, DependencyObject);
impl windows_core::RuntimeType for AutomationPeer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutomationPeer>();
}
unsafe impl windows_core::Interface for AutomationPeer {
    type Vtable = <IAutomationPeer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAutomationPeer as windows_core::Interface>::IID;
}
impl core::ops::Deref for AutomationPeer {
    type Target = IAutomationPeer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AutomationPeer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Automation.Peers.AutomationPeer";
}
unsafe impl Send for AutomationPeer {}
unsafe impl Sync for AutomationPeer {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationProperties(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    AutomationProperties,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl AutomationProperties {
    pub fn SetName<P0>(element: P0, value: &str) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IAutomationPropertiesStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetName)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        })
    }
    fn IAutomationPropertiesStatics<
        R,
        F: FnOnce(&IAutomationPropertiesStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            AutomationProperties,
            IAutomationPropertiesStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for AutomationProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IAutomationProperties>();
}
unsafe impl windows_core::Interface for AutomationProperties {
    type Vtable = <IAutomationProperties as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IAutomationProperties as windows_core::Interface>::IID;
}
impl core::ops::Deref for AutomationProperties {
    type Target = IAutomationProperties;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for AutomationProperties {
    const NAME: &'static str = "Microsoft.UI.Xaml.Automation.AutomationProperties";
}
unsafe impl Send for AutomationProperties {}
unsafe impl Sync for AutomationProperties {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BitmapAlphaMode(pub i32);
impl BitmapAlphaMode {
    pub const Premultiplied: Self = Self(0);
    pub const Straight: Self = Self(1);
    pub const Ignore: Self = Self(2);
}
impl windows_core::imp::TypeKind for BitmapAlphaMode {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for BitmapAlphaMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Graphics.Imaging.BitmapAlphaMode;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapEncoder(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    BitmapEncoder,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl BitmapEncoder {
    pub fn PngEncoderId() -> windows_core::Result<windows_core::GUID> {
        Self::IBitmapEncoderStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).PngEncoderId)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| result__)
        })
    }
    pub fn CreateAsync<P1>(
        encoderid: windows_core::GUID,
        stream: P1,
    ) -> windows_core::Result<windows_future::IAsyncOperation<Self>>
    where
        P1: windows_core::Param<IRandomAccessStream>,
    {
        Self::IBitmapEncoderStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateAsync)(
                windows_core::Interface::as_raw(this),
                encoderid,
                stream.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IBitmapEncoderStatics<R, F: FnOnce(&IBitmapEncoderStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<BitmapEncoder, IBitmapEncoderStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for BitmapEncoder {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBitmapEncoder>();
}
unsafe impl windows_core::Interface for BitmapEncoder {
    type Vtable = <IBitmapEncoder as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBitmapEncoder as windows_core::Interface>::IID;
}
impl core::ops::Deref for BitmapEncoder {
    type Target = IBitmapEncoder;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for BitmapEncoder {
    const NAME: &'static str = "Windows.Graphics.Imaging.BitmapEncoder";
}
unsafe impl Send for BitmapEncoder {}
unsafe impl Sync for BitmapEncoder {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapImage(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    BitmapImage,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(BitmapImage, BitmapSource, ImageSource, DependencyObject);
impl BitmapImage {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            BitmapImage,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for BitmapImage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBitmapImage>();
}
unsafe impl windows_core::Interface for BitmapImage {
    type Vtable = <IBitmapImage as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBitmapImage as windows_core::Interface>::IID;
}
impl core::ops::Deref for BitmapImage {
    type Target = IBitmapImage;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for BitmapImage {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Imaging.BitmapImage";
}
unsafe impl Send for BitmapImage {}
unsafe impl Sync for BitmapImage {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BitmapPixelFormat(pub i32);
impl BitmapPixelFormat {
    pub const Unknown: Self = Self(0);
    pub const Rgba16: Self = Self(12);
    pub const Rgba8: Self = Self(30);
    pub const Gray16: Self = Self(57);
    pub const Gray8: Self = Self(62);
    pub const Bgra8: Self = Self(87);
    pub const Nv12: Self = Self(103);
    pub const P010: Self = Self(104);
    pub const Yuy2: Self = Self(107);
}
impl windows_core::imp::TypeKind for BitmapPixelFormat {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for BitmapPixelFormat {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Graphics.Imaging.BitmapPixelFormat;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BitmapSource(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    BitmapSource,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(BitmapSource, ImageSource, DependencyObject);
impl windows_core::RuntimeType for BitmapSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBitmapSource>();
}
unsafe impl windows_core::Interface for BitmapSource {
    type Vtable = <IBitmapSource as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBitmapSource as windows_core::Interface>::IID;
}
impl core::ops::Deref for BitmapSource {
    type Target = IBitmapSource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for BitmapSource {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Imaging.BitmapSource";
}
unsafe impl Send for BitmapSource {}
unsafe impl Sync for BitmapSource {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Buffer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Buffer,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IBuffer
);
impl Buffer {
    pub fn Create(capacity: u32) -> windows_core::Result<Self> {
        Self::IBufferFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Create)(
                windows_core::Interface::as_raw(this),
                capacity,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IBufferFactory<R, F: FnOnce(&IBufferFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Buffer, IBufferFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Buffer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IBuffer>();
}
unsafe impl windows_core::Interface for Buffer {
    type Vtable = <IBuffer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IBuffer as windows_core::Interface>::IID;
}
impl core::ops::Deref for Buffer {
    type Target = IBuffer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Buffer {
    const NAME: &'static str = "Windows.Storage.Streams.Buffer";
}
unsafe impl Send for Buffer {}
unsafe impl Sync for Buffer {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Button(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Button, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(
    Button,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for Button {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButton>();
}
unsafe impl windows_core::Interface for Button {
    type Vtable = <IButton as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButton as windows_core::Interface>::IID;
}
impl core::ops::Deref for Button {
    type Target = IButton;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Button {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Button";
}
unsafe impl Send for Button {}
unsafe impl Sync for Button {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ButtonBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ButtonBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IButtonBase>();
}
unsafe impl windows_core::Interface for ButtonBase {
    type Vtable = <IButtonBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IButtonBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for ButtonBase {
    type Target = IButtonBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ButtonBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.ButtonBase";
}
unsafe impl Send for ButtonBase {}
unsafe impl Sync for ButtonBase {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CERT_CONTEXT {
    pub dwCertEncodingType: u32,
    pub pbCertEncoded: *mut u8,
    pub cbCertEncoded: u32,
    pub pCertInfo: PCERT_INFO,
    pub hCertStore: HCERTSTORE,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CERT_EXTENSION {
    pub pszObjId: windows_core::PSTR,
    pub fCritical: windows_core::BOOL,
    pub Value: CRYPT_OBJID_BLOB,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CERT_INFO {
    pub dwVersion: u32,
    pub SerialNumber: CRYPT_INTEGER_BLOB,
    pub SignatureAlgorithm: CRYPT_ALGORITHM_IDENTIFIER,
    pub Issuer: CERT_NAME_BLOB,
    pub NotBefore: FILETIME,
    pub NotAfter: FILETIME,
    pub Subject: CERT_NAME_BLOB,
    pub SubjectPublicKeyInfo: CERT_PUBLIC_KEY_INFO,
    pub IssuerUniqueId: CRYPT_BIT_BLOB,
    pub SubjectUniqueId: CRYPT_BIT_BLOB,
    pub cExtension: u32,
    pub rgExtension: PCERT_EXTENSION,
}
pub type CERT_NAME_BLOB = CRYPT_INTEGER_BLOB;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CERT_PUBLIC_KEY_INFO {
    pub Algorithm: CRYPT_ALGORITHM_IDENTIFIER,
    pub PublicKey: CRYPT_BIT_BLOB,
}
pub const CERT_STORE_ADD_ALWAYS: i32 = 4;
pub const CERT_STORE_CERTIFICATE_CONTEXT: i32 = 1;
pub const CERT_STORE_PROV_MEMORY: windows_core::PCSTR = windows_core::PCSTR(2 as _);
pub type COINIT = i32;
pub const COINIT_APARTMENTTHREADED: COINIT = 2;
pub const COINIT_MULTITHREADED: COINIT = 0;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CRYPT_ALGORITHM_IDENTIFIER {
    pub pszObjId: windows_core::PSTR,
    pub Parameters: CRYPT_OBJID_BLOB,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CRYPT_BIT_BLOB {
    pub cbData: u32,
    pub pbData: *mut u8,
    pub cUnusedBits: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CRYPT_INTEGER_BLOB {
    pub cbData: u32,
    pub pbData: *mut u8,
}
pub type CRYPT_OBJID_BLOB = CRYPT_INTEGER_BLOB;
pub struct Clipboard;
impl Clipboard {
    pub fn GetContent() -> windows_core::Result<DataPackageView> {
        Self::IClipboardStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetContent)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn SetContent<P0>(content: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DataPackage>,
    {
        Self::IClipboardStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetContent)(
                windows_core::Interface::as_raw(this),
                content.param().abi(),
            )
            .ok()
        })
    }
    pub fn Flush() -> windows_core::Result<()> {
        Self::IClipboardStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).Flush)(windows_core::Interface::as_raw(this))
                .ok()
        })
    }
    fn IClipboardStatics<R, F: FnOnce(&IClipboardStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Clipboard, IClipboardStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeName for Clipboard {
    const NAME: &'static str = "Windows.ApplicationModel.DataTransfer.Clipboard";
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnDefinition(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ColumnDefinition,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ColumnDefinition, DependencyObject);
impl windows_core::RuntimeType for ColumnDefinition {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IColumnDefinition>();
}
unsafe impl windows_core::Interface for ColumnDefinition {
    type Vtable = <IColumnDefinition as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IColumnDefinition as windows_core::Interface>::IID;
}
impl core::ops::Deref for ColumnDefinition {
    type Target = IColumnDefinition;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ColumnDefinition {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ColumnDefinition";
}
unsafe impl Send for ColumnDefinition {}
unsafe impl Sync for ColumnDefinition {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ColumnDefinitionCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ColumnDefinitionCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IVector<ColumnDefinition>
);
impl windows_core::RuntimeType for ColumnDefinitionCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        windows_collections::IVector<ColumnDefinition>,
    >();
}
unsafe impl windows_core::Interface for ColumnDefinitionCollection {
    type Vtable =
        <windows_collections::IVector<ColumnDefinition> as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <windows_collections::IVector<ColumnDefinition> as windows_core::Interface>::IID;
}
impl core::ops::Deref for ColumnDefinitionCollection {
    type Target = windows_collections::IVector<ColumnDefinition>;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ColumnDefinitionCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ColumnDefinitionCollection";
}
unsafe impl Send for ColumnDefinitionCollection {}
unsafe impl Sync for ColumnDefinitionCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComboBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ComboBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ComboBox,
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ComboBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IComboBox>();
}
unsafe impl windows_core::Interface for ComboBox {
    type Vtable = <IComboBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IComboBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for ComboBox {
    type Target = IComboBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ComboBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ComboBox";
}
unsafe impl Send for ComboBox {}
unsafe impl Sync for ComboBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerContentChangingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContainerContentChangingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for ContainerContentChangingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContainerContentChangingEventArgs>();
}
unsafe impl windows_core::Interface for ContainerContentChangingEventArgs {
    type Vtable = <IContainerContentChangingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IContainerContentChangingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for ContainerContentChangingEventArgs {
    type Target = IContainerContentChangingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ContainerContentChangingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContainerContentChangingEventArgs";
}
unsafe impl Send for ContainerContentChangingEventArgs {}
unsafe impl Sync for ContainerContentChangingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentControl>();
}
unsafe impl windows_core::Interface for ContentControl {
    type Vtable = <IContentControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for ContentControl {
    type Target = IContentControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ContentControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContentControl";
}
unsafe impl Send for ContentControl {}
unsafe impl Sync for ContentControl {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContentDialog(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ContentDialog,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ContentDialog,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ContentDialog {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IContentDialog>();
}
unsafe impl windows_core::Interface for ContentDialog {
    type Vtable = <IContentDialog as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IContentDialog as windows_core::Interface>::IID;
}
impl core::ops::Deref for ContentDialog {
    type Target = IContentDialog;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ContentDialog {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ContentDialog";
}
unsafe impl Send for ContentDialog {}
unsafe impl Sync for ContentDialog {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentDialogResult(pub i32);
impl ContentDialogResult {
    pub const None: Self = Self(0);
    pub const Primary: Self = Self(1);
    pub const Secondary: Self = Self(2);
}
impl windows_core::imp::TypeKind for ContentDialogResult {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for ContentDialogResult {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.ContentDialogResult;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Control(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Control,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Control, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Control {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IControl>();
}
unsafe impl windows_core::Interface for Control {
    type Vtable = <IControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for Control {
    type Target = IControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Control {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Control";
}
unsafe impl Send for Control {}
unsafe impl Sync for Control {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2>();
}
unsafe impl windows_core::Interface for CoreWebView2 {
    type Vtable = <ICoreWebView2 as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2 as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2 {
    type Target = ICoreWebView2;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2 {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2";
}
unsafe impl Send for CoreWebView2 {}
unsafe impl Sync for CoreWebView2 {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2BrowserExtension(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2BrowserExtension,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2BrowserExtension {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2BrowserExtension>();
}
unsafe impl windows_core::Interface for CoreWebView2BrowserExtension {
    type Vtable = <ICoreWebView2BrowserExtension as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2BrowserExtension as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2BrowserExtension {
    type Target = ICoreWebView2BrowserExtension;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2BrowserExtension {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2BrowserExtension";
}
unsafe impl Send for CoreWebView2BrowserExtension {}
unsafe impl Sync for CoreWebView2BrowserExtension {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2BrowsingDataKinds(pub u32);
impl CoreWebView2BrowsingDataKinds {
    pub const FileSystems: Self = Self(1);
    pub const IndexedDb: Self = Self(2);
    pub const LocalStorage: Self = Self(4);
    pub const WebSql: Self = Self(8);
    pub const CacheStorage: Self = Self(16);
    pub const AllDomStorage: Self = Self(32);
    pub const Cookies: Self = Self(64);
    pub const AllSite: Self = Self(128);
    pub const DiskCache: Self = Self(256);
    pub const DownloadHistory: Self = Self(512);
    pub const GeneralAutofill: Self = Self(1024);
    pub const PasswordAutosave: Self = Self(2048);
    pub const BrowsingHistory: Self = Self(4096);
    pub const Settings: Self = Self(8192);
    pub const AllProfile: Self = Self(16384);
    pub const ServiceWorkers: Self = Self(32768);
}
impl windows_core::imp::TypeKind for CoreWebView2BrowsingDataKinds {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2BrowsingDataKinds {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2BrowsingDataKinds;u4)",
    );
}
impl CoreWebView2BrowsingDataKinds {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for CoreWebView2BrowsingDataKinds {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for CoreWebView2BrowsingDataKinds {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for CoreWebView2BrowsingDataKinds {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for CoreWebView2BrowsingDataKinds {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for CoreWebView2BrowsingDataKinds {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2CapturePreviewImageFormat(pub i32);
impl CoreWebView2CapturePreviewImageFormat {
    pub const Png: Self = Self(0);
    pub const Jpeg: Self = Self(1);
}
impl windows_core::imp::TypeKind for CoreWebView2CapturePreviewImageFormat {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2CapturePreviewImageFormat {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2CapturePreviewImageFormat;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ContentLoadingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ContentLoadingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ContentLoadingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2ContentLoadingEventArgs>();
}
unsafe impl windows_core::Interface for CoreWebView2ContentLoadingEventArgs {
    type Vtable = <ICoreWebView2ContentLoadingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2ContentLoadingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ContentLoadingEventArgs {
    type Target = ICoreWebView2ContentLoadingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ContentLoadingEventArgs {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2ContentLoadingEventArgs";
}
unsafe impl Send for CoreWebView2ContentLoadingEventArgs {}
unsafe impl Sync for CoreWebView2ContentLoadingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ContextMenuItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ContextMenuItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ContextMenuItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2ContextMenuItem>();
}
unsafe impl windows_core::Interface for CoreWebView2ContextMenuItem {
    type Vtable = <ICoreWebView2ContextMenuItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2ContextMenuItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ContextMenuItem {
    type Target = ICoreWebView2ContextMenuItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ContextMenuItem {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2ContextMenuItem";
}
unsafe impl Send for CoreWebView2ContextMenuItem {}
unsafe impl Sync for CoreWebView2ContextMenuItem {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2ContextMenuItemKind(pub i32);
impl CoreWebView2ContextMenuItemKind {
    pub const Command: Self = Self(0);
    pub const CheckBox: Self = Self(1);
    pub const Radio: Self = Self(2);
    pub const Separator: Self = Self(3);
    pub const Submenu: Self = Self(4);
}
impl windows_core::imp::TypeKind for CoreWebView2ContextMenuItemKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2ContextMenuItemKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2ContextMenuItemKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ContextMenuRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ContextMenuRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ContextMenuRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2ContextMenuRequestedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2ContextMenuRequestedEventArgs {
    type Vtable = <ICoreWebView2ContextMenuRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2ContextMenuRequestedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ContextMenuRequestedEventArgs {
    type Target = ICoreWebView2ContextMenuRequestedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ContextMenuRequestedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2ContextMenuRequestedEventArgs";
}
unsafe impl Send for CoreWebView2ContextMenuRequestedEventArgs {}
unsafe impl Sync for CoreWebView2ContextMenuRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ContextMenuTarget(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ContextMenuTarget,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ContextMenuTarget {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2ContextMenuTarget>();
}
unsafe impl windows_core::Interface for CoreWebView2ContextMenuTarget {
    type Vtable = <ICoreWebView2ContextMenuTarget as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2ContextMenuTarget as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ContextMenuTarget {
    type Target = ICoreWebView2ContextMenuTarget;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ContextMenuTarget {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2ContextMenuTarget";
}
unsafe impl Send for CoreWebView2ContextMenuTarget {}
unsafe impl Sync for CoreWebView2ContextMenuTarget {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2ContextMenuTargetKind(pub i32);
impl CoreWebView2ContextMenuTargetKind {
    pub const Page: Self = Self(0);
    pub const Image: Self = Self(1);
    pub const SelectedText: Self = Self(2);
    pub const Audio: Self = Self(3);
    pub const Video: Self = Self(4);
}
impl windows_core::imp::TypeKind for CoreWebView2ContextMenuTargetKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2ContextMenuTargetKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2ContextMenuTargetKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2DevToolsProtocolEventReceivedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2DevToolsProtocolEventReceivedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2DevToolsProtocolEventReceivedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2DevToolsProtocolEventReceivedEventArgs {
    type Vtable =
        <ICoreWebView2DevToolsProtocolEventReceivedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2DevToolsProtocolEventReceivedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2DevToolsProtocolEventReceivedEventArgs {
    type Target = ICoreWebView2DevToolsProtocolEventReceivedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2DevToolsProtocolEventReceivedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2DevToolsProtocolEventReceivedEventArgs";
}
unsafe impl Send for CoreWebView2DevToolsProtocolEventReceivedEventArgs {}
unsafe impl Sync for CoreWebView2DevToolsProtocolEventReceivedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2DevToolsProtocolEventReceiver(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2DevToolsProtocolEventReceiver,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2DevToolsProtocolEventReceiver {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2DevToolsProtocolEventReceiver,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2DevToolsProtocolEventReceiver {
    type Vtable = <ICoreWebView2DevToolsProtocolEventReceiver as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2DevToolsProtocolEventReceiver as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2DevToolsProtocolEventReceiver {
    type Target = ICoreWebView2DevToolsProtocolEventReceiver;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2DevToolsProtocolEventReceiver {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2DevToolsProtocolEventReceiver";
}
unsafe impl Send for CoreWebView2DevToolsProtocolEventReceiver {}
unsafe impl Sync for CoreWebView2DevToolsProtocolEventReceiver {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2DownloadInterruptReason(pub i32);
impl CoreWebView2DownloadInterruptReason {
    pub const None: Self = Self(0);
    pub const FileFailed: Self = Self(1);
    pub const FileAccessDenied: Self = Self(2);
    pub const FileNoSpace: Self = Self(3);
    pub const FileNameTooLong: Self = Self(4);
    pub const FileTooLarge: Self = Self(5);
    pub const FileMalicious: Self = Self(6);
    pub const FileTransientError: Self = Self(7);
    pub const FileBlockedByPolicy: Self = Self(8);
    pub const FileSecurityCheckFailed: Self = Self(9);
    pub const FileTooShort: Self = Self(10);
    pub const FileHashMismatch: Self = Self(11);
    pub const NetworkFailed: Self = Self(12);
    pub const NetworkTimeout: Self = Self(13);
    pub const NetworkDisconnected: Self = Self(14);
    pub const NetworkServerDown: Self = Self(15);
    pub const NetworkInvalidRequest: Self = Self(16);
    pub const ServerFailed: Self = Self(17);
    pub const ServerNoRange: Self = Self(18);
    pub const ServerBadContent: Self = Self(19);
    pub const ServerUnauthorized: Self = Self(20);
    pub const ServerCertificateProblem: Self = Self(21);
    pub const ServerForbidden: Self = Self(22);
    pub const ServerUnexpectedResponse: Self = Self(23);
    pub const ServerContentLengthMismatch: Self = Self(24);
    pub const ServerCrossOriginRedirect: Self = Self(25);
    pub const UserCanceled: Self = Self(26);
    pub const UserShutdown: Self = Self(27);
    pub const UserPaused: Self = Self(28);
    pub const DownloadProcessCrashed: Self = Self(29);
}
impl windows_core::imp::TypeKind for CoreWebView2DownloadInterruptReason {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2DownloadInterruptReason {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2DownloadInterruptReason;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2DownloadOperation(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2DownloadOperation,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2DownloadOperation {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2DownloadOperation>();
}
unsafe impl windows_core::Interface for CoreWebView2DownloadOperation {
    type Vtable = <ICoreWebView2DownloadOperation as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2DownloadOperation as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2DownloadOperation {
    type Target = ICoreWebView2DownloadOperation;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2DownloadOperation {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2DownloadOperation";
}
unsafe impl Send for CoreWebView2DownloadOperation {}
unsafe impl Sync for CoreWebView2DownloadOperation {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2DownloadStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2DownloadStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2DownloadStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2DownloadStartingEventArgs>();
}
unsafe impl windows_core::Interface for CoreWebView2DownloadStartingEventArgs {
    type Vtable = <ICoreWebView2DownloadStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2DownloadStartingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2DownloadStartingEventArgs {
    type Target = ICoreWebView2DownloadStartingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2DownloadStartingEventArgs {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2DownloadStartingEventArgs";
}
unsafe impl Send for CoreWebView2DownloadStartingEventArgs {}
unsafe impl Sync for CoreWebView2DownloadStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2DownloadState(pub i32);
impl CoreWebView2DownloadState {
    pub const InProgress: Self = Self(0);
    pub const Interrupted: Self = Self(1);
    pub const Completed: Self = Self(2);
}
impl windows_core::imp::TypeKind for CoreWebView2DownloadState {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2DownloadState {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2DownloadState;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2Environment(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2Environment,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl CoreWebView2Environment {
    pub fn CreateWithOptionsAsync<P2>(
        browserexecutablefolder: &str,
        userdatafolder: &str,
        options: P2,
    ) -> windows_core::Result<windows_future::IAsyncOperation<Self>>
    where
        P2: windows_core::Param<CoreWebView2EnvironmentOptions>,
    {
        Self::ICoreWebView2EnvironmentStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateWithOptionsAsync)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(browserexecutablefolder)),
                core::mem::transmute_copy(&windows_core::HSTRING::from(userdatafolder)),
                options.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn GetAvailableBrowserVersionString() -> windows_core::Result<String> {
        Self::ICoreWebView2EnvironmentStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetAvailableBrowserVersionString)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        })
    }
    pub fn GetAvailableBrowserVersionString2(
        browserexecutablefolder: &str,
    ) -> windows_core::Result<String> {
        Self::ICoreWebView2EnvironmentStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetAvailableBrowserVersionString2)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(browserexecutablefolder)),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        })
    }
    fn ICoreWebView2EnvironmentStatics<
        R,
        F: FnOnce(&ICoreWebView2EnvironmentStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            CoreWebView2Environment,
            ICoreWebView2EnvironmentStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for CoreWebView2Environment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2Environment>();
}
unsafe impl windows_core::Interface for CoreWebView2Environment {
    type Vtable = <ICoreWebView2Environment as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2Environment as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2Environment {
    type Target = ICoreWebView2Environment;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2Environment {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2Environment";
}
unsafe impl Send for CoreWebView2Environment {}
unsafe impl Sync for CoreWebView2Environment {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2EnvironmentOptions(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2EnvironmentOptions,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2EnvironmentOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2EnvironmentOptions>();
}
unsafe impl windows_core::Interface for CoreWebView2EnvironmentOptions {
    type Vtable = <ICoreWebView2EnvironmentOptions as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2EnvironmentOptions as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2EnvironmentOptions {
    type Target = ICoreWebView2EnvironmentOptions;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2EnvironmentOptions {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2EnvironmentOptions";
}
unsafe impl Send for CoreWebView2EnvironmentOptions {}
unsafe impl Sync for CoreWebView2EnvironmentOptions {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2FaviconImageFormat(pub i32);
impl CoreWebView2FaviconImageFormat {
    pub const Png: Self = Self(0);
    pub const Jpeg: Self = Self(1);
}
impl windows_core::imp::TypeKind for CoreWebView2FaviconImageFormat {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2FaviconImageFormat {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2FaviconImageFormat;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2Find(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2Find,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2Find {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2Find>();
}
unsafe impl windows_core::Interface for CoreWebView2Find {
    type Vtable = <ICoreWebView2Find as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2Find as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2Find {
    type Target = ICoreWebView2Find;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2Find {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2Find";
}
unsafe impl Send for CoreWebView2Find {}
unsafe impl Sync for CoreWebView2Find {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2FindOptions(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2FindOptions,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2FindOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2FindOptions>();
}
unsafe impl windows_core::Interface for CoreWebView2FindOptions {
    type Vtable = <ICoreWebView2FindOptions as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2FindOptions as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2FindOptions {
    type Target = ICoreWebView2FindOptions;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2FindOptions {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2FindOptions";
}
unsafe impl Send for CoreWebView2FindOptions {}
unsafe impl Sync for CoreWebView2FindOptions {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2FrameInfo(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2FrameInfo,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2FrameInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2FrameInfo>();
}
unsafe impl windows_core::Interface for CoreWebView2FrameInfo {
    type Vtable = <ICoreWebView2FrameInfo as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2FrameInfo as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2FrameInfo {
    type Target = ICoreWebView2FrameInfo;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2FrameInfo {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2FrameInfo";
}
unsafe impl Send for CoreWebView2FrameInfo {}
unsafe impl Sync for CoreWebView2FrameInfo {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2NavigationCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2NavigationCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2NavigationCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2NavigationCompletedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2NavigationCompletedEventArgs {
    type Vtable = <ICoreWebView2NavigationCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2NavigationCompletedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2NavigationCompletedEventArgs {
    type Target = ICoreWebView2NavigationCompletedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2NavigationCompletedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2NavigationCompletedEventArgs";
}
unsafe impl Send for CoreWebView2NavigationCompletedEventArgs {}
unsafe impl Sync for CoreWebView2NavigationCompletedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2NavigationStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2NavigationStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2NavigationStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2NavigationStartingEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2NavigationStartingEventArgs {
    type Vtable = <ICoreWebView2NavigationStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2NavigationStartingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2NavigationStartingEventArgs {
    type Target = ICoreWebView2NavigationStartingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2NavigationStartingEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2NavigationStartingEventArgs";
}
unsafe impl Send for CoreWebView2NavigationStartingEventArgs {}
unsafe impl Sync for CoreWebView2NavigationStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2NewWindowRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2NewWindowRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2NewWindowRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2NewWindowRequestedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2NewWindowRequestedEventArgs {
    type Vtable = <ICoreWebView2NewWindowRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2NewWindowRequestedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2NewWindowRequestedEventArgs {
    type Target = ICoreWebView2NewWindowRequestedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2NewWindowRequestedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2NewWindowRequestedEventArgs";
}
unsafe impl Send for CoreWebView2NewWindowRequestedEventArgs {}
unsafe impl Sync for CoreWebView2NewWindowRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2PermissionKind(pub i32);
impl CoreWebView2PermissionKind {
    pub const UnknownPermission: Self = Self(0);
    pub const Microphone: Self = Self(1);
    pub const Camera: Self = Self(2);
    pub const Geolocation: Self = Self(3);
    pub const Notifications: Self = Self(4);
    pub const OtherSensors: Self = Self(5);
    pub const ClipboardRead: Self = Self(6);
    pub const MultipleAutomaticDownloads: Self = Self(7);
    pub const FileReadWrite: Self = Self(8);
    pub const Autoplay: Self = Self(9);
    pub const LocalFonts: Self = Self(10);
    pub const MidiSystemExclusiveMessages: Self = Self(11);
    pub const WindowManagement: Self = Self(12);
    pub const PersistentStorage: Self = Self(13);
}
impl windows_core::imp::TypeKind for CoreWebView2PermissionKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2PermissionKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2PermissionKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2PermissionRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2PermissionRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2PermissionRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2PermissionRequestedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2PermissionRequestedEventArgs {
    type Vtable = <ICoreWebView2PermissionRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2PermissionRequestedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2PermissionRequestedEventArgs {
    type Target = ICoreWebView2PermissionRequestedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2PermissionRequestedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2PermissionRequestedEventArgs";
}
unsafe impl Send for CoreWebView2PermissionRequestedEventArgs {}
unsafe impl Sync for CoreWebView2PermissionRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2PermissionSetting(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2PermissionSetting,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2PermissionSetting {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2PermissionSetting>();
}
unsafe impl windows_core::Interface for CoreWebView2PermissionSetting {
    type Vtable = <ICoreWebView2PermissionSetting as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2PermissionSetting as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2PermissionSetting {
    type Target = ICoreWebView2PermissionSetting;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2PermissionSetting {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2PermissionSetting";
}
unsafe impl Send for CoreWebView2PermissionSetting {}
unsafe impl Sync for CoreWebView2PermissionSetting {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2PermissionState(pub i32);
impl CoreWebView2PermissionState {
    pub const Default: Self = Self(0);
    pub const Allow: Self = Self(1);
    pub const Deny: Self = Self(2);
}
impl windows_core::imp::TypeKind for CoreWebView2PermissionState {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2PermissionState {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2PermissionState;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2PrintDialogKind(pub i32);
impl CoreWebView2PrintDialogKind {
    pub const Browser: Self = Self(0);
    pub const System: Self = Self(1);
}
impl windows_core::imp::TypeKind for CoreWebView2PrintDialogKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2PrintDialogKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2PrintDialogKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ProcessFailedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ProcessFailedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ProcessFailedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2ProcessFailedEventArgs>();
}
unsafe impl windows_core::Interface for CoreWebView2ProcessFailedEventArgs {
    type Vtable = <ICoreWebView2ProcessFailedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2ProcessFailedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ProcessFailedEventArgs {
    type Target = ICoreWebView2ProcessFailedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ProcessFailedEventArgs {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2ProcessFailedEventArgs";
}
unsafe impl Send for CoreWebView2ProcessFailedEventArgs {}
unsafe impl Sync for CoreWebView2ProcessFailedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2ProcessFailedKind(pub i32);
impl CoreWebView2ProcessFailedKind {
    pub const BrowserProcessExited: Self = Self(0);
    pub const RenderProcessExited: Self = Self(1);
    pub const RenderProcessUnresponsive: Self = Self(2);
    pub const FrameRenderProcessExited: Self = Self(3);
    pub const UtilityProcessExited: Self = Self(4);
    pub const SandboxHelperProcessExited: Self = Self(5);
    pub const GpuProcessExited: Self = Self(6);
    pub const PpapiPluginProcessExited: Self = Self(7);
    pub const PpapiBrokerProcessExited: Self = Self(8);
    pub const UnknownProcessExited: Self = Self(9);
}
impl windows_core::imp::TypeKind for CoreWebView2ProcessFailedKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2ProcessFailedKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2ProcessFailedKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2Profile(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2Profile,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2Profile {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2Profile>();
}
unsafe impl windows_core::Interface for CoreWebView2Profile {
    type Vtable = <ICoreWebView2Profile as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2Profile as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2Profile {
    type Target = ICoreWebView2Profile;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2Profile {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2Profile";
}
unsafe impl Send for CoreWebView2Profile {}
unsafe impl Sync for CoreWebView2Profile {}
windows_core::imp::define_interface!(
    CoreWebView2Profile_Manual2,
    CoreWebView2Profile_Manual2_Vtbl,
    0x6e62815a_6269_5756_92c3_f08afe17649c
);
impl windows_core::RuntimeType for CoreWebView2Profile_Manual2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl CoreWebView2Profile_Manual2 {
    pub fn GetNonDefaultPermissionSettingsAsync(
        &self,
    ) -> windows_core::Result<
        windows_future::IAsyncOperation<
            windows_collections::IVectorView<CoreWebView2PermissionSetting>,
        >,
    > {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetNonDefaultPermissionSettingsAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct CoreWebView2Profile_Manual2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetNonDefaultPermissionSettingsAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    )
        -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    CoreWebView2Profile_Manual3,
    CoreWebView2Profile_Manual3_Vtbl,
    0xc6129971_9ecc_5634_8896_723c1dbacd6f
);
impl windows_core::RuntimeType for CoreWebView2Profile_Manual3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl CoreWebView2Profile_Manual3 {
    pub fn GetBrowserExtensionsAsync(
        &self,
    ) -> windows_core::Result<
        windows_future::IAsyncOperation<
            windows_collections::IVectorView<CoreWebView2BrowserExtension>,
        >,
    > {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetBrowserExtensionsAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct CoreWebView2Profile_Manual3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetBrowserExtensionsAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2SaveAsKind(pub i32);
impl CoreWebView2SaveAsKind {
    pub const Default: Self = Self(0);
    pub const HtmlOnly: Self = Self(1);
    pub const SingleFile: Self = Self(2);
    pub const Complete: Self = Self(3);
}
impl windows_core::imp::TypeKind for CoreWebView2SaveAsKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2SaveAsKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2SaveAsKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2SaveAsUIResult(pub i32);
impl CoreWebView2SaveAsUIResult {
    pub const Success: Self = Self(0);
    pub const InvalidPath: Self = Self(1);
    pub const FileAlreadyExists: Self = Self(2);
    pub const KindNotSupported: Self = Self(3);
    pub const Cancelled: Self = Self(4);
}
impl windows_core::imp::TypeKind for CoreWebView2SaveAsUIResult {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2SaveAsUIResult {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2SaveAsUIResult;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2SaveAsUIShowingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2SaveAsUIShowingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2SaveAsUIShowingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2SaveAsUIShowingEventArgs>();
}
unsafe impl windows_core::Interface for CoreWebView2SaveAsUIShowingEventArgs {
    type Vtable = <ICoreWebView2SaveAsUIShowingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2SaveAsUIShowingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2SaveAsUIShowingEventArgs {
    type Target = ICoreWebView2SaveAsUIShowingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2SaveAsUIShowingEventArgs {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2SaveAsUIShowingEventArgs";
}
unsafe impl Send for CoreWebView2SaveAsUIShowingEventArgs {}
unsafe impl Sync for CoreWebView2SaveAsUIShowingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2ScreenCaptureStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2ScreenCaptureStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2ScreenCaptureStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2ScreenCaptureStartingEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2ScreenCaptureStartingEventArgs {
    type Vtable = <ICoreWebView2ScreenCaptureStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2ScreenCaptureStartingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2ScreenCaptureStartingEventArgs {
    type Target = ICoreWebView2ScreenCaptureStartingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2ScreenCaptureStartingEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2ScreenCaptureStartingEventArgs";
}
unsafe impl Send for CoreWebView2ScreenCaptureStartingEventArgs {}
unsafe impl Sync for CoreWebView2ScreenCaptureStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2Settings(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2Settings,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2Settings {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2Settings>();
}
unsafe impl windows_core::Interface for CoreWebView2Settings {
    type Vtable = <ICoreWebView2Settings as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ICoreWebView2Settings as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2Settings {
    type Target = ICoreWebView2Settings;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2Settings {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2Settings";
}
unsafe impl Send for CoreWebView2Settings {}
unsafe impl Sync for CoreWebView2Settings {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2SourceChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2SourceChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2SourceChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2SourceChangedEventArgs>();
}
unsafe impl windows_core::Interface for CoreWebView2SourceChangedEventArgs {
    type Vtable = <ICoreWebView2SourceChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2SourceChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2SourceChangedEventArgs {
    type Target = ICoreWebView2SourceChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2SourceChangedEventArgs {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2SourceChangedEventArgs";
}
unsafe impl Send for CoreWebView2SourceChangedEventArgs {}
unsafe impl Sync for CoreWebView2SourceChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2TrackingPreventionLevel(pub i32);
impl CoreWebView2TrackingPreventionLevel {
    pub const None: Self = Self(0);
    pub const Basic: Self = Self(1);
    pub const Balanced: Self = Self(2);
    pub const Strict: Self = Self(3);
}
impl windows_core::imp::TypeKind for CoreWebView2TrackingPreventionLevel {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2TrackingPreventionLevel {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2TrackingPreventionLevel;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2WebErrorStatus(pub i32);
impl CoreWebView2WebErrorStatus {
    pub const Unknown: Self = Self(0);
    pub const CertificateCommonNameIsIncorrect: Self = Self(1);
    pub const CertificateExpired: Self = Self(2);
    pub const ClientCertificateContainsErrors: Self = Self(3);
    pub const CertificateRevoked: Self = Self(4);
    pub const CertificateIsInvalid: Self = Self(5);
    pub const ServerUnreachable: Self = Self(6);
    pub const Timeout: Self = Self(7);
    pub const ErrorHttpInvalidServerResponse: Self = Self(8);
    pub const ConnectionAborted: Self = Self(9);
    pub const ConnectionReset: Self = Self(10);
    pub const Disconnected: Self = Self(11);
    pub const CannotConnect: Self = Self(12);
    pub const HostNameNotResolved: Self = Self(13);
    pub const OperationCanceled: Self = Self(14);
    pub const RedirectFailed: Self = Self(15);
    pub const UnexpectedError: Self = Self(16);
    pub const ValidAuthenticationCredentialsRequired: Self = Self(17);
    pub const ValidProxyAuthenticationRequired: Self = Self(18);
}
impl windows_core::imp::TypeKind for CoreWebView2WebErrorStatus {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2WebErrorStatus {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2WebErrorStatus;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2WebMessageReceivedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2WebMessageReceivedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2WebMessageReceivedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2WebMessageReceivedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2WebMessageReceivedEventArgs {
    type Vtable = <ICoreWebView2WebMessageReceivedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2WebMessageReceivedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2WebMessageReceivedEventArgs {
    type Target = ICoreWebView2WebMessageReceivedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2WebMessageReceivedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2WebMessageReceivedEventArgs";
}
unsafe impl Send for CoreWebView2WebMessageReceivedEventArgs {}
unsafe impl Sync for CoreWebView2WebMessageReceivedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2WebResourceContext(pub i32);
impl CoreWebView2WebResourceContext {
    pub const All: Self = Self(0);
    pub const Document: Self = Self(1);
    pub const Stylesheet: Self = Self(2);
    pub const Image: Self = Self(3);
    pub const Media: Self = Self(4);
    pub const Font: Self = Self(5);
    pub const Script: Self = Self(6);
    pub const XmlHttpRequest: Self = Self(7);
    pub const Fetch: Self = Self(8);
    pub const TextTrack: Self = Self(9);
    pub const EventSource: Self = Self(10);
    pub const Websocket: Self = Self(11);
    pub const Manifest: Self = Self(12);
    pub const SignedExchange: Self = Self(13);
    pub const Ping: Self = Self(14);
    pub const CspViolationReport: Self = Self(15);
    pub const Other: Self = Self(16);
}
impl windows_core::imp::TypeKind for CoreWebView2WebResourceContext {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2WebResourceContext {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2WebResourceContext;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2WebResourceRequest(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2WebResourceRequest,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2WebResourceRequest {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2WebResourceRequest>();
}
unsafe impl windows_core::Interface for CoreWebView2WebResourceRequest {
    type Vtable = <ICoreWebView2WebResourceRequest as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2WebResourceRequest as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2WebResourceRequest {
    type Target = ICoreWebView2WebResourceRequest;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2WebResourceRequest {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2WebResourceRequest";
}
unsafe impl Send for CoreWebView2WebResourceRequest {}
unsafe impl Sync for CoreWebView2WebResourceRequest {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CoreWebView2WebResourceRequestSourceKinds(pub u32);
impl CoreWebView2WebResourceRequestSourceKinds {
    pub const None: Self = Self(0);
    pub const Document: Self = Self(1);
    pub const SharedWorker: Self = Self(2);
    pub const ServiceWorker: Self = Self(4);
    pub const All: Self = Self(4294967295);
}
impl windows_core::imp::TypeKind for CoreWebView2WebResourceRequestSourceKinds {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for CoreWebView2WebResourceRequestSourceKinds {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Web.WebView2.Core.CoreWebView2WebResourceRequestSourceKinds;u4)",
    );
}
impl CoreWebView2WebResourceRequestSourceKinds {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for CoreWebView2WebResourceRequestSourceKinds {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for CoreWebView2WebResourceRequestSourceKinds {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for CoreWebView2WebResourceRequestSourceKinds {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for CoreWebView2WebResourceRequestSourceKinds {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for CoreWebView2WebResourceRequestSourceKinds {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2WebResourceRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2WebResourceRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2WebResourceRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        ICoreWebView2WebResourceRequestedEventArgs,
    >();
}
unsafe impl windows_core::Interface for CoreWebView2WebResourceRequestedEventArgs {
    type Vtable = <ICoreWebView2WebResourceRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2WebResourceRequestedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2WebResourceRequestedEventArgs {
    type Target = ICoreWebView2WebResourceRequestedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2WebResourceRequestedEventArgs {
    const NAME: &'static str =
        "Microsoft.Web.WebView2.Core.CoreWebView2WebResourceRequestedEventArgs";
}
unsafe impl Send for CoreWebView2WebResourceRequestedEventArgs {}
unsafe impl Sync for CoreWebView2WebResourceRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoreWebView2WebResourceResponse(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    CoreWebView2WebResourceResponse,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for CoreWebView2WebResourceResponse {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ICoreWebView2WebResourceResponse>();
}
unsafe impl windows_core::Interface for CoreWebView2WebResourceResponse {
    type Vtable = <ICoreWebView2WebResourceResponse as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ICoreWebView2WebResourceResponse as windows_core::Interface>::IID;
}
impl core::ops::Deref for CoreWebView2WebResourceResponse {
    type Target = ICoreWebView2WebResourceResponse;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for CoreWebView2WebResourceResponse {
    const NAME: &'static str = "Microsoft.Web.WebView2.Core.CoreWebView2WebResourceResponse";
}
unsafe impl Send for CoreWebView2WebResourceResponse {}
unsafe impl Sync for CoreWebView2WebResourceResponse {}
pub type CreatePackageDependencyOptions = u32;
pub const CreatePackageDependencyOptions_DoNotVerifyDependencyResolution:
    CreatePackageDependencyOptions = 1;
pub const CreatePackageDependencyOptions_None: CreatePackageDependencyOptions = 0;
pub const CreatePackageDependencyOptions_ScopeIsSystem: CreatePackageDependencyOptions = 2;
pub struct CryptographicBuffer;
impl CryptographicBuffer {
    pub fn CreateFromByteArray(value: &[u8]) -> windows_core::Result<IBuffer> {
        Self::ICryptographicBufferStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateFromByteArray)(
                windows_core::Interface::as_raw(this),
                value.len().try_into().unwrap(),
                value.as_ptr(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ICryptographicBufferStatics<
        R,
        F: FnOnce(&ICryptographicBufferStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            CryptographicBuffer,
            ICryptographicBufferStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeName for CryptographicBuffer {
    const NAME: &'static str = "Windows.Security.Cryptography.CryptographicBuffer";
}
pub const D3D11_CREATE_DEVICE_BGRA_SUPPORT: D3D11_CREATE_DEVICE_FLAG = 32;
pub type D3D11_CREATE_DEVICE_FLAG = i32;
pub const D3D11_SDK_VERSION: i32 = 7;
pub type D3D_DRIVER_TYPE = i32;
pub const D3D_DRIVER_TYPE_HARDWARE: D3D_DRIVER_TYPE = 1;
pub type D3D_FEATURE_LEVEL = i32;
pub type DPI_AWARENESS_CONTEXT = *mut core::ffi::c_void;
pub const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: DPI_AWARENESS_CONTEXT = -4 as _;
pub const DWMWA_EXTENDED_FRAME_BOUNDS: DWMWINDOWATTRIBUTE = 9;
pub type DWMWINDOWATTRIBUTE = i32;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPackage(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataPackage,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DataPackage {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DataPackage,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DataPackage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataPackage>();
}
unsafe impl windows_core::Interface for DataPackage {
    type Vtable = <IDataPackage as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataPackage as windows_core::Interface>::IID;
}
impl core::ops::Deref for DataPackage {
    type Target = IDataPackage;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DataPackage {
    const NAME: &'static str = "Windows.ApplicationModel.DataTransfer.DataPackage";
}
unsafe impl Send for DataPackage {}
unsafe impl Sync for DataPackage {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataPackageView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataPackageView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for DataPackageView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataPackageView>();
}
unsafe impl windows_core::Interface for DataPackageView {
    type Vtable = <IDataPackageView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataPackageView as windows_core::Interface>::IID;
}
impl core::ops::Deref for DataPackageView {
    type Target = IDataPackageView;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DataPackageView {
    const NAME: &'static str = "Windows.ApplicationModel.DataTransfer.DataPackageView";
}
unsafe impl Send for DataPackageView {}
unsafe impl Sync for DataPackageView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataReader(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataReader,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IDataReader
);
impl DataReader {
    pub fn CreateDataReader<P0>(inputstream: P0) -> windows_core::Result<Self>
    where
        P0: windows_core::Param<IInputStream>,
    {
        Self::IDataReaderFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateDataReader)(
                windows_core::Interface::as_raw(this),
                inputstream.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn FromBuffer<P0>(buffer: P0) -> windows_core::Result<Self>
    where
        P0: windows_core::Param<IBuffer>,
    {
        Self::IDataReaderStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).FromBuffer)(
                windows_core::Interface::as_raw(this),
                buffer.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDataReaderFactory<R, F: FnOnce(&IDataReaderFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DataReader, IDataReaderFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
    fn IDataReaderStatics<R, F: FnOnce(&IDataReaderStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DataReader, IDataReaderStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DataReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataReader>();
}
unsafe impl windows_core::Interface for DataReader {
    type Vtable = <IDataReader as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataReader as windows_core::Interface>::IID;
}
impl core::ops::Deref for DataReader {
    type Target = IDataReader;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DataReader {
    const NAME: &'static str = "Windows.Storage.Streams.DataReader";
}
unsafe impl Send for DataReader {}
unsafe impl Sync for DataReader {}
pub type DataReaderLoadOperation = windows_future::IAsyncOperation<u32>;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataWriter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DataWriter,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IDataWriter
);
impl DataWriter {
    pub fn CreateDataWriter<P0>(outputstream: P0) -> windows_core::Result<Self>
    where
        P0: windows_core::Param<IOutputStream>,
    {
        Self::IDataWriterFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateDataWriter)(
                windows_core::Interface::as_raw(this),
                outputstream.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDataWriterFactory<R, F: FnOnce(&IDataWriterFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DataWriter, IDataWriterFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DataWriter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDataWriter>();
}
unsafe impl windows_core::Interface for DataWriter {
    type Vtable = <IDataWriter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDataWriter as windows_core::Interface>::IID;
}
impl core::ops::Deref for DataWriter {
    type Target = IDataWriter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DataWriter {
    const NAME: &'static str = "Windows.Storage.Streams.DataWriter";
}
unsafe impl Send for DataWriter {}
unsafe impl Sync for DataWriter {}
pub type DataWriterStoreOperation = windows_future::IAsyncOperation<u32>;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DateTime {
    pub universal_time: i64,
}
impl windows_core::imp::TypeKind for DateTime {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for DateTime {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.DateTime;i8)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateTimeFormatter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DateTimeFormatter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DateTimeFormatter {
    pub fn CreateDateTimeFormatter(formattemplate: &str) -> windows_core::Result<Self> {
        Self::IDateTimeFormatterFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateDateTimeFormatter)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(formattemplate)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDateTimeFormatterFactory<
        R,
        F: FnOnce(&IDateTimeFormatterFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            DateTimeFormatter,
            IDateTimeFormatterFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DateTimeFormatter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDateTimeFormatter>();
}
unsafe impl windows_core::Interface for DateTimeFormatter {
    type Vtable = <IDateTimeFormatter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDateTimeFormatter as windows_core::Interface>::IID;
}
impl core::ops::Deref for DateTimeFormatter {
    type Target = IDateTimeFormatter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DateTimeFormatter {
    const NAME: &'static str = "Windows.Globalization.DateTimeFormatting.DateTimeFormatter";
}
unsafe impl Send for DateTimeFormatter {}
unsafe impl Sync for DateTimeFormatter {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deferral(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Deferral,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for Deferral {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDeferral>();
}
unsafe impl windows_core::Interface for Deferral {
    type Vtable = <IDeferral as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDeferral as windows_core::Interface>::IID;
}
impl core::ops::Deref for Deferral {
    type Target = IDeferral;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Deferral {
    const NAME: &'static str = "Windows.Foundation.Deferral";
}
unsafe impl Send for Deferral {}
unsafe impl Sync for Deferral {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyObject(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DependencyObject,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for DependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDependencyObject>();
}
unsafe impl windows_core::Interface for DependencyObject {
    type Vtable = <IDependencyObject as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDependencyObject as windows_core::Interface>::IID;
}
impl core::ops::Deref for DependencyObject {
    type Target = IDependencyObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DependencyObject {
    const NAME: &'static str = "Microsoft.UI.Xaml.DependencyObject";
}
unsafe impl Send for DependencyObject {}
unsafe impl Sync for DependencyObject {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Direct3D11CaptureFrame(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Direct3D11CaptureFrame,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for Direct3D11CaptureFrame {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDirect3D11CaptureFrame>();
}
unsafe impl windows_core::Interface for Direct3D11CaptureFrame {
    type Vtable = <IDirect3D11CaptureFrame as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDirect3D11CaptureFrame as windows_core::Interface>::IID;
}
impl core::ops::Deref for Direct3D11CaptureFrame {
    type Target = IDirect3D11CaptureFrame;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Direct3D11CaptureFrame {
    const NAME: &'static str = "Windows.Graphics.Capture.Direct3D11CaptureFrame";
}
unsafe impl Send for Direct3D11CaptureFrame {}
unsafe impl Sync for Direct3D11CaptureFrame {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Direct3D11CaptureFramePool(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Direct3D11CaptureFramePool,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl Direct3D11CaptureFramePool {
    pub fn CreateFreeThreaded<P0>(
        device: P0,
        pixelformat: DirectXPixelFormat,
        numberofbuffers: i32,
        size: SizeInt32,
    ) -> windows_core::Result<Self>
    where
        P0: windows_core::Param<IDirect3DDevice>,
    {
        Self::IDirect3D11CaptureFramePoolStatics2(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateFreeThreaded)(
                windows_core::Interface::as_raw(this),
                device.param().abi(),
                pixelformat,
                numberofbuffers,
                size,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDirect3D11CaptureFramePoolStatics2<
        R,
        F: FnOnce(&IDirect3D11CaptureFramePoolStatics2) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            Direct3D11CaptureFramePool,
            IDirect3D11CaptureFramePoolStatics2,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Direct3D11CaptureFramePool {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDirect3D11CaptureFramePool>();
}
unsafe impl windows_core::Interface for Direct3D11CaptureFramePool {
    type Vtable = <IDirect3D11CaptureFramePool as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDirect3D11CaptureFramePool as windows_core::Interface>::IID;
}
impl core::ops::Deref for Direct3D11CaptureFramePool {
    type Target = IDirect3D11CaptureFramePool;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Direct3D11CaptureFramePool {
    const NAME: &'static str = "Windows.Graphics.Capture.Direct3D11CaptureFramePool";
}
unsafe impl Send for Direct3D11CaptureFramePool {}
unsafe impl Sync for Direct3D11CaptureFramePool {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirectXPixelFormat(pub i32);
impl DirectXPixelFormat {
    pub const Unknown: Self = Self(0);
    pub const R32G32B32A32Typeless: Self = Self(1);
    pub const R32G32B32A32Float: Self = Self(2);
    pub const R32G32B32A32UInt: Self = Self(3);
    pub const R32G32B32A32Int: Self = Self(4);
    pub const R32G32B32Typeless: Self = Self(5);
    pub const R32G32B32Float: Self = Self(6);
    pub const R32G32B32UInt: Self = Self(7);
    pub const R32G32B32Int: Self = Self(8);
    pub const R16G16B16A16Typeless: Self = Self(9);
    pub const R16G16B16A16Float: Self = Self(10);
    pub const R16G16B16A16UIntNormalized: Self = Self(11);
    pub const R16G16B16A16UInt: Self = Self(12);
    pub const R16G16B16A16IntNormalized: Self = Self(13);
    pub const R16G16B16A16Int: Self = Self(14);
    pub const R32G32Typeless: Self = Self(15);
    pub const R32G32Float: Self = Self(16);
    pub const R32G32UInt: Self = Self(17);
    pub const R32G32Int: Self = Self(18);
    pub const R32G8X24Typeless: Self = Self(19);
    pub const D32FloatS8X24UInt: Self = Self(20);
    pub const R32FloatX8X24Typeless: Self = Self(21);
    pub const X32TypelessG8X24UInt: Self = Self(22);
    pub const R10G10B10A2Typeless: Self = Self(23);
    pub const R10G10B10A2UIntNormalized: Self = Self(24);
    pub const R10G10B10A2UInt: Self = Self(25);
    pub const R11G11B10Float: Self = Self(26);
    pub const R8G8B8A8Typeless: Self = Self(27);
    pub const R8G8B8A8UIntNormalized: Self = Self(28);
    pub const R8G8B8A8UIntNormalizedSrgb: Self = Self(29);
    pub const R8G8B8A8UInt: Self = Self(30);
    pub const R8G8B8A8IntNormalized: Self = Self(31);
    pub const R8G8B8A8Int: Self = Self(32);
    pub const R16G16Typeless: Self = Self(33);
    pub const R16G16Float: Self = Self(34);
    pub const R16G16UIntNormalized: Self = Self(35);
    pub const R16G16UInt: Self = Self(36);
    pub const R16G16IntNormalized: Self = Self(37);
    pub const R16G16Int: Self = Self(38);
    pub const R32Typeless: Self = Self(39);
    pub const D32Float: Self = Self(40);
    pub const R32Float: Self = Self(41);
    pub const R32UInt: Self = Self(42);
    pub const R32Int: Self = Self(43);
    pub const R24G8Typeless: Self = Self(44);
    pub const D24UIntNormalizedS8UInt: Self = Self(45);
    pub const R24UIntNormalizedX8Typeless: Self = Self(46);
    pub const X24TypelessG8UInt: Self = Self(47);
    pub const R8G8Typeless: Self = Self(48);
    pub const R8G8UIntNormalized: Self = Self(49);
    pub const R8G8UInt: Self = Self(50);
    pub const R8G8IntNormalized: Self = Self(51);
    pub const R8G8Int: Self = Self(52);
    pub const R16Typeless: Self = Self(53);
    pub const R16Float: Self = Self(54);
    pub const D16UIntNormalized: Self = Self(55);
    pub const R16UIntNormalized: Self = Self(56);
    pub const R16UInt: Self = Self(57);
    pub const R16IntNormalized: Self = Self(58);
    pub const R16Int: Self = Self(59);
    pub const R8Typeless: Self = Self(60);
    pub const R8UIntNormalized: Self = Self(61);
    pub const R8UInt: Self = Self(62);
    pub const R8IntNormalized: Self = Self(63);
    pub const R8Int: Self = Self(64);
    pub const A8UIntNormalized: Self = Self(65);
    pub const R1UIntNormalized: Self = Self(66);
    pub const R9G9B9E5SharedExponent: Self = Self(67);
    pub const R8G8B8G8UIntNormalized: Self = Self(68);
    pub const G8R8G8B8UIntNormalized: Self = Self(69);
    pub const BC1Typeless: Self = Self(70);
    pub const BC1UIntNormalized: Self = Self(71);
    pub const BC1UIntNormalizedSrgb: Self = Self(72);
    pub const BC2Typeless: Self = Self(73);
    pub const BC2UIntNormalized: Self = Self(74);
    pub const BC2UIntNormalizedSrgb: Self = Self(75);
    pub const BC3Typeless: Self = Self(76);
    pub const BC3UIntNormalized: Self = Self(77);
    pub const BC3UIntNormalizedSrgb: Self = Self(78);
    pub const BC4Typeless: Self = Self(79);
    pub const BC4UIntNormalized: Self = Self(80);
    pub const BC4IntNormalized: Self = Self(81);
    pub const BC5Typeless: Self = Self(82);
    pub const BC5UIntNormalized: Self = Self(83);
    pub const BC5IntNormalized: Self = Self(84);
    pub const B5G6R5UIntNormalized: Self = Self(85);
    pub const B5G5R5A1UIntNormalized: Self = Self(86);
    pub const B8G8R8A8UIntNormalized: Self = Self(87);
    pub const B8G8R8X8UIntNormalized: Self = Self(88);
    pub const R10G10B10XRBiasA2UIntNormalized: Self = Self(89);
    pub const B8G8R8A8Typeless: Self = Self(90);
    pub const B8G8R8A8UIntNormalizedSrgb: Self = Self(91);
    pub const B8G8R8X8Typeless: Self = Self(92);
    pub const B8G8R8X8UIntNormalizedSrgb: Self = Self(93);
    pub const BC6HTypeless: Self = Self(94);
    pub const BC6H16UnsignedFloat: Self = Self(95);
    pub const BC6H16Float: Self = Self(96);
    pub const BC7Typeless: Self = Self(97);
    pub const BC7UIntNormalized: Self = Self(98);
    pub const BC7UIntNormalizedSrgb: Self = Self(99);
    pub const Ayuv: Self = Self(100);
    pub const Y410: Self = Self(101);
    pub const Y416: Self = Self(102);
    pub const NV12: Self = Self(103);
    pub const P010: Self = Self(104);
    pub const P016: Self = Self(105);
    pub const Opaque420: Self = Self(106);
    pub const Yuy2: Self = Self(107);
    pub const Y210: Self = Self(108);
    pub const Y216: Self = Self(109);
    pub const NV11: Self = Self(110);
    pub const AI44: Self = Self(111);
    pub const IA44: Self = Self(112);
    pub const P8: Self = Self(113);
    pub const A8P8: Self = Self(114);
    pub const B4G4R4A4UIntNormalized: Self = Self(115);
    pub const P208: Self = Self(130);
    pub const V208: Self = Self(131);
    pub const V408: Self = Self(132);
    pub const SamplerFeedbackMinMipOpaque: Self = Self(189);
    pub const SamplerFeedbackMipRegionUsedOpaque: Self = Self(190);
    pub const A4B4G4R4: Self = Self(191);
}
impl windows_core::imp::TypeKind for DirectXPixelFormat {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for DirectXPixelFormat {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Windows.Graphics.DirectX.DirectXPixelFormat;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatcherQueue(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DispatcherQueue,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DispatcherQueue {
    pub fn GetForCurrentThread() -> windows_core::Result<Self> {
        Self::IDispatcherQueueStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetForCurrentThread)(
                windows_core::Interface::as_raw(this),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDispatcherQueueStatics<
        R,
        F: FnOnce(&IDispatcherQueueStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DispatcherQueue, IDispatcherQueueStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DispatcherQueue {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDispatcherQueue>();
}
unsafe impl windows_core::Interface for DispatcherQueue {
    type Vtable = <IDispatcherQueue as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDispatcherQueue as windows_core::Interface>::IID;
}
impl core::ops::Deref for DispatcherQueue {
    type Target = IDispatcherQueue;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DispatcherQueue {
    const NAME: &'static str = "Microsoft.UI.Dispatching.DispatcherQueue";
}
unsafe impl Send for DispatcherQueue {}
unsafe impl Sync for DispatcherQueue {}
windows_core::imp::define_interface!(
    DispatcherQueueHandler,
    DispatcherQueueHandler_Vtbl,
    0x2e0872a9_4e29_5f14_b688_fb96d5f9d5f8
);
impl windows_core::RuntimeType for DispatcherQueueHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl DispatcherQueueHandler {
    pub fn new<F: Fn() + 'static>(invoke: F) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &DispatcherQueueHandlerBox::<F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
}
#[repr(C)]
pub struct DispatcherQueueHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(this: *mut core::ffi::c_void) -> windows_core::HRESULT,
}
struct DispatcherQueueHandlerBox<F: Fn() + 'static>(core::marker::PhantomData<(fn() -> F,)>);
impl<F: Fn() + 'static> DispatcherQueueHandlerBox<F> {
    const VTABLE: DispatcherQueueHandler_Vtbl = DispatcherQueueHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<DispatcherQueueHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(this: *mut core::ffi::c_void) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<DispatcherQueueHandler, F>);
            (this.invoke)();
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayArea(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DisplayArea,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl DisplayArea {
    pub fn GetFromRect(
        rect: RectInt32,
        displayareafallback: DisplayAreaFallback,
    ) -> windows_core::Result<Self> {
        Self::IDisplayAreaStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetFromRect)(
                windows_core::Interface::as_raw(this),
                rect,
                displayareafallback,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IDisplayAreaStatics<R, F: FnOnce(&IDisplayAreaStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<DisplayArea, IDisplayAreaStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for DisplayArea {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDisplayArea>();
}
unsafe impl windows_core::Interface for DisplayArea {
    type Vtable = <IDisplayArea as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDisplayArea as windows_core::Interface>::IID;
}
impl core::ops::Deref for DisplayArea {
    type Target = IDisplayArea;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DisplayArea {
    const NAME: &'static str = "Microsoft.UI.Windowing.DisplayArea";
}
unsafe impl Send for DisplayArea {}
unsafe impl Sync for DisplayArea {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DisplayAreaFallback(pub i32);
impl DisplayAreaFallback {
    pub const None: Self = Self(0);
    pub const Primary: Self = Self(1);
    pub const Nearest: Self = Self(2);
}
impl windows_core::imp::TypeKind for DisplayAreaFallback {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for DisplayAreaFallback {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Windowing.DisplayAreaFallback;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DoubleAnimation(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DoubleAnimation,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(DoubleAnimation, Timeline, DependencyObject);
impl windows_core::RuntimeType for DoubleAnimation {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDoubleAnimation>();
}
unsafe impl windows_core::Interface for DoubleAnimation {
    type Vtable = <IDoubleAnimation as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDoubleAnimation as windows_core::Interface>::IID;
}
impl core::ops::Deref for DoubleAnimation {
    type Target = IDoubleAnimation;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DoubleAnimation {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Animation.DoubleAnimation";
}
unsafe impl Send for DoubleAnimation {}
unsafe impl Sync for DoubleAnimation {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DragItemsCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    DragItemsCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for DragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IDragItemsCompletedEventArgs>();
}
unsafe impl windows_core::Interface for DragItemsCompletedEventArgs {
    type Vtable = <IDragItemsCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IDragItemsCompletedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for DragItemsCompletedEventArgs {
    type Target = IDragItemsCompletedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for DragItemsCompletedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.DragItemsCompletedEventArgs";
}
unsafe impl Send for DragItemsCompletedEventArgs {}
unsafe impl Sync for DragItemsCompletedEventArgs {}
pub const E_FAIL: windows_core::HRESULT = windows_core::HRESULT(0x80004005_u32 as _);
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ElementTheme(pub i32);
impl ElementTheme {
    pub const Default: Self = Self(0);
    pub const Light: Self = Self(1);
    pub const Dark: Self = Self(2);
}
impl windows_core::imp::TypeKind for ElementTheme {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for ElementTheme {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.ElementTheme;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventHandler<T>(windows_core::IUnknown, core::marker::PhantomData<T>)
where
    T: windows_core::RuntimeType + 'static;
unsafe impl<T: windows_core::RuntimeType + 'static> windows_core::Interface for EventHandler<T> {
    type Vtable = EventHandler_Vtbl<T>;
    const IID: windows_core::GUID =
        windows_core::GUID::from_signature(<Self as windows_core::RuntimeType>::SIGNATURE);
}
impl<T: windows_core::RuntimeType + 'static> windows_core::RuntimeType for EventHandler<T> {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::new()
        .push_slice(b"pinterface({9de1c535-6ae1-11e0-84e1-18a905bcc53f}")
        .push_slice(b";")
        .push_other(T::SIGNATURE)
        .push_slice(b")");
}
#[repr(C)]
pub struct EventHandler_Vtbl<T>
where
    T: windows_core::RuntimeType + 'static,
{
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        args: windows_core::imp::AbiType<T>,
    ) -> windows_core::HRESULT,
    T: core::marker::PhantomData<T>,
}
struct EventHandlerBox<
    T,
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<T>) + 'static,
>(core::marker::PhantomData<(T, fn() -> F)>)
where
    T: windows_core::RuntimeType + 'static;
impl<
    T: windows_core::RuntimeType + 'static,
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<T>) + 'static,
> EventHandlerBox<T, F>
{
    const VTABLE: EventHandler_Vtbl<T> = EventHandler_Vtbl::<T> {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<EventHandler<T>, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<EventHandler<T>, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<EventHandler<T>, F>::Release,
        },
        Invoke: Self::Invoke,
        T: core::marker::PhantomData::<T>,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        args: windows_core::imp::AbiType<T>,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<EventHandler<T>, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&args),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExtendedActivationKind(pub i32);
impl ExtendedActivationKind {
    pub const Launch: Self = Self(0);
    pub const Search: Self = Self(1);
    pub const ShareTarget: Self = Self(2);
    pub const File: Self = Self(3);
    pub const Protocol: Self = Self(4);
    pub const FileOpenPicker: Self = Self(5);
    pub const FileSavePicker: Self = Self(6);
    pub const CachedFileUpdater: Self = Self(7);
    pub const ContactPicker: Self = Self(8);
    pub const Device: Self = Self(9);
    pub const PrintTaskSettings: Self = Self(10);
    pub const CameraSettings: Self = Self(11);
    pub const RestrictedLaunch: Self = Self(12);
    pub const AppointmentsProvider: Self = Self(13);
    pub const Contact: Self = Self(14);
    pub const LockScreenCall: Self = Self(15);
    pub const VoiceCommand: Self = Self(16);
    pub const LockScreen: Self = Self(17);
    pub const PickerReturned: Self = Self(1000);
    pub const WalletAction: Self = Self(1001);
    pub const PickFileContinuation: Self = Self(1002);
    pub const PickSaveFileContinuation: Self = Self(1003);
    pub const PickFolderContinuation: Self = Self(1004);
    pub const WebAuthenticationBrokerContinuation: Self = Self(1005);
    pub const WebAccountProvider: Self = Self(1006);
    pub const ComponentUI: Self = Self(1007);
    pub const ProtocolForResults: Self = Self(1009);
    pub const ToastNotification: Self = Self(1010);
    pub const Print3DWorkflow: Self = Self(1011);
    pub const DialReceiver: Self = Self(1012);
    pub const DevicePairing: Self = Self(1013);
    pub const UserDataAccountsProvider: Self = Self(1014);
    pub const FilePickerExperience: Self = Self(1015);
    pub const LockScreenComponent: Self = Self(1016);
    pub const ContactPanel: Self = Self(1017);
    pub const PrintWorkflowForegroundTask: Self = Self(1018);
    pub const GameUIProvider: Self = Self(1019);
    pub const StartupTask: Self = Self(1020);
    pub const CommandLineLaunch: Self = Self(1021);
    pub const BarcodeScannerProvider: Self = Self(1022);
    pub const PrintSupportJobUI: Self = Self(1023);
    pub const PrintSupportSettingsUI: Self = Self(1024);
    pub const PhoneCallActivation: Self = Self(1025);
    pub const VpnForeground: Self = Self(1026);
    pub const Push: Self = Self(5000);
    pub const AppNotification: Self = Self(5001);
}
impl windows_core::imp::TypeKind for ExtendedActivationKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for ExtendedActivationKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.Windows.AppLifecycle.ExtendedActivationKind;i4)",
    );
}
pub type FARPROC = Option<unsafe extern "system" fn() -> isize>;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FILETIME {
    pub dwLowDateTime: u32,
    pub dwHighDateTime: u32,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileOpenPicker(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FileOpenPicker,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FileOpenPicker {
    pub fn CreateInstance(windowid: WindowId) -> windows_core::Result<Self> {
        Self::IFileOpenPickerFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                windowid,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFileOpenPickerFactory<R, F: FnOnce(&IFileOpenPickerFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FileOpenPicker, IFileOpenPickerFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FileOpenPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFileOpenPicker>();
}
unsafe impl windows_core::Interface for FileOpenPicker {
    type Vtable = <IFileOpenPicker as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFileOpenPicker as windows_core::Interface>::IID;
}
impl core::ops::Deref for FileOpenPicker {
    type Target = IFileOpenPicker;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FileOpenPicker {
    const NAME: &'static str = "Microsoft.Windows.Storage.Pickers.FileOpenPicker";
}
unsafe impl Send for FileOpenPicker {}
unsafe impl Sync for FileOpenPicker {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileSavePicker(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FileSavePicker,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FileSavePicker {
    pub fn CreateInstance(windowid: WindowId) -> windows_core::Result<Self> {
        Self::IFileSavePickerFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                windowid,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFileSavePickerFactory<R, F: FnOnce(&IFileSavePickerFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FileSavePicker, IFileSavePickerFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FileSavePicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFileSavePicker>();
}
unsafe impl windows_core::Interface for FileSavePicker {
    type Vtable = <IFileSavePicker as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFileSavePicker as windows_core::Interface>::IID;
}
impl core::ops::Deref for FileSavePicker {
    type Target = IFileSavePicker;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FileSavePicker {
    const NAME: &'static str = "Microsoft.Windows.Storage.Pickers.FileSavePicker";
}
unsafe impl Send for FileSavePicker {}
unsafe impl Sync for FileSavePicker {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Flyout(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Flyout, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Flyout, FlyoutBase, DependencyObject);
impl windows_core::RuntimeType for Flyout {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFlyout>();
}
unsafe impl windows_core::Interface for Flyout {
    type Vtable = <IFlyout as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFlyout as windows_core::Interface>::IID;
}
impl core::ops::Deref for Flyout {
    type Target = IFlyout;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Flyout {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Flyout";
}
unsafe impl Send for Flyout {}
unsafe impl Sync for Flyout {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlyoutBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FlyoutBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FlyoutBase, DependencyObject);
impl windows_core::RuntimeType for FlyoutBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFlyoutBase>();
}
unsafe impl windows_core::Interface for FlyoutBase {
    type Vtable = <IFlyoutBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFlyoutBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for FlyoutBase {
    type Target = IFlyoutBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FlyoutBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.FlyoutBase";
}
unsafe impl Send for FlyoutBase {}
unsafe impl Sync for FlyoutBase {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlyoutPlacementMode(pub i32);
impl FlyoutPlacementMode {
    pub const Top: Self = Self(0);
    pub const Bottom: Self = Self(1);
    pub const Left: Self = Self(2);
    pub const Right: Self = Self(3);
    pub const Full: Self = Self(4);
    pub const TopEdgeAlignedLeft: Self = Self(5);
    pub const TopEdgeAlignedRight: Self = Self(6);
    pub const BottomEdgeAlignedLeft: Self = Self(7);
    pub const BottomEdgeAlignedRight: Self = Self(8);
    pub const LeftEdgeAlignedTop: Self = Self(9);
    pub const LeftEdgeAlignedBottom: Self = Self(10);
    pub const RightEdgeAlignedTop: Self = Self(11);
    pub const RightEdgeAlignedBottom: Self = Self(12);
    pub const Auto: Self = Self(13);
}
impl windows_core::imp::TypeKind for FlyoutPlacementMode {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for FlyoutPlacementMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.Primitives.FlyoutPlacementMode;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlyoutShowMode(pub i32);
impl FlyoutShowMode {
    pub const Auto: Self = Self(0);
    pub const Standard: Self = Self(1);
    pub const Transient: Self = Self(2);
    pub const TransientWithDismissOnPointerMoveAway: Self = Self(3);
}
impl windows_core::imp::TypeKind for FlyoutShowMode {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for FlyoutShowMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowMode;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FlyoutShowOptions(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FlyoutShowOptions,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FlyoutShowOptions {
    pub fn new() -> windows_core::Result<Self> {
        Self::IFlyoutShowOptionsFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFlyoutShowOptionsFactory<
        R,
        F: FnOnce(&IFlyoutShowOptionsFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            FlyoutShowOptions,
            IFlyoutShowOptionsFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FlyoutShowOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFlyoutShowOptions>();
}
unsafe impl windows_core::Interface for FlyoutShowOptions {
    type Vtable = <IFlyoutShowOptions as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFlyoutShowOptions as windows_core::Interface>::IID;
}
impl core::ops::Deref for FlyoutShowOptions {
    type Target = IFlyoutShowOptions;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FlyoutShowOptions {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.FlyoutShowOptions";
}
unsafe impl Send for FlyoutShowOptions {}
unsafe impl Sync for FlyoutShowOptions {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusManager(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FocusManager,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FocusManager {
    pub fn GetFocusedElementWithRoot<P0>(
        xamlroot: P0,
    ) -> windows_core::Result<windows_core::IInspectable>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        Self::IFocusManagerStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetFocusedElementWithRoot)(
                windows_core::Interface::as_raw(this),
                xamlroot.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFocusManagerStatics<R, F: FnOnce(&IFocusManagerStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FocusManager, IFocusManagerStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FocusManager {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFocusManager>();
}
unsafe impl windows_core::Interface for FocusManager {
    type Vtable = <IFocusManager as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFocusManager as windows_core::Interface>::IID;
}
impl core::ops::Deref for FocusManager {
    type Target = IFocusManager;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FocusManager {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.FocusManager";
}
unsafe impl Send for FocusManager {}
unsafe impl Sync for FocusManager {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FocusState(pub i32);
impl FocusState {
    pub const Unfocused: Self = Self(0);
    pub const Pointer: Self = Self(1);
    pub const Keyboard: Self = Self(2);
    pub const Programmatic: Self = Self(3);
}
impl windows_core::imp::TypeKind for FocusState {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for FocusState {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.FocusState;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderPicker(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FolderPicker,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl FolderPicker {
    pub fn CreateInstance(windowid: WindowId) -> windows_core::Result<Self> {
        Self::IFolderPickerFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                windowid,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFolderPickerFactory<R, F: FnOnce(&IFolderPickerFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FolderPicker, IFolderPickerFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FolderPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFolderPicker>();
}
unsafe impl windows_core::Interface for FolderPicker {
    type Vtable = <IFolderPicker as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFolderPicker as windows_core::Interface>::IID;
}
impl core::ops::Deref for FolderPicker {
    type Target = IFolderPicker;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FolderPicker {
    const NAME: &'static str = "Microsoft.Windows.Storage.Pickers.FolderPicker";
}
unsafe impl Send for FolderPicker {}
unsafe impl Sync for FolderPicker {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontIcon(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FontIcon,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    FontIcon,
    IconElement,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl FontIcon {
    pub fn new() -> windows_core::Result<Self> {
        Self::IFontIconFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFontIconFactory<R, F: FnOnce(&IFontIconFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<FontIcon, IFontIconFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FontIcon {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFontIcon>();
}
unsafe impl windows_core::Interface for FontIcon {
    type Vtable = <IFontIcon as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFontIcon as windows_core::Interface>::IID;
}
impl core::ops::Deref for FontIcon {
    type Target = IFontIcon;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FontIcon {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.FontIcon";
}
unsafe impl Send for FontIcon {}
unsafe impl Sync for FontIcon {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FrameworkElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for FrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFrameworkElement>();
}
unsafe impl windows_core::Interface for FrameworkElement {
    type Vtable = <IFrameworkElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IFrameworkElement as windows_core::Interface>::IID;
}
impl core::ops::Deref for FrameworkElement {
    type Target = IFrameworkElement;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FrameworkElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.FrameworkElement";
}
unsafe impl Send for FrameworkElement {}
unsafe impl Sync for FrameworkElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkElementAutomationPeer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    FrameworkElementAutomationPeer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    FrameworkElementAutomationPeer,
    AutomationPeer,
    DependencyObject
);
impl FrameworkElementAutomationPeer {
    pub fn CreatePeerForElement<P0>(element: P0) -> windows_core::Result<AutomationPeer>
    where
        P0: windows_core::Param<UIElement>,
    {
        Self::IFrameworkElementAutomationPeerStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreatePeerForElement)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IFrameworkElementAutomationPeerStatics<
        R,
        F: FnOnce(&IFrameworkElementAutomationPeerStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            FrameworkElementAutomationPeer,
            IFrameworkElementAutomationPeerStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for FrameworkElementAutomationPeer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IFrameworkElementAutomationPeer>();
}
unsafe impl windows_core::Interface for FrameworkElementAutomationPeer {
    type Vtable = <IFrameworkElementAutomationPeer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IFrameworkElementAutomationPeer as windows_core::Interface>::IID;
}
impl core::ops::Deref for FrameworkElementAutomationPeer {
    type Target = IFrameworkElementAutomationPeer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for FrameworkElementAutomationPeer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Automation.Peers.FrameworkElementAutomationPeer";
}
unsafe impl Send for FrameworkElementAutomationPeer {}
unsafe impl Sync for FrameworkElementAutomationPeer {}
pub const GA_ROOTOWNER: i32 = 3;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneralTransform(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    GeneralTransform,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(GeneralTransform, DependencyObject);
impl windows_core::RuntimeType for GeneralTransform {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IGeneralTransform>();
}
unsafe impl windows_core::Interface for GeneralTransform {
    type Vtable = <IGeneralTransform as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IGeneralTransform as windows_core::Interface>::IID;
}
impl core::ops::Deref for GeneralTransform {
    type Target = IGeneralTransform;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for GeneralTransform {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.GeneralTransform";
}
unsafe impl Send for GeneralTransform {}
unsafe impl Sync for GeneralTransform {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsCaptureItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    GraphicsCaptureItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for GraphicsCaptureItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IGraphicsCaptureItem>();
}
unsafe impl windows_core::Interface for GraphicsCaptureItem {
    type Vtable = <IGraphicsCaptureItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IGraphicsCaptureItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for GraphicsCaptureItem {
    type Target = IGraphicsCaptureItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for GraphicsCaptureItem {
    const NAME: &'static str = "Windows.Graphics.Capture.GraphicsCaptureItem";
}
unsafe impl Send for GraphicsCaptureItem {}
unsafe impl Sync for GraphicsCaptureItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphicsCaptureSession(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    GraphicsCaptureSession,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for GraphicsCaptureSession {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IGraphicsCaptureSession>();
}
unsafe impl windows_core::Interface for GraphicsCaptureSession {
    type Vtable = <IGraphicsCaptureSession as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IGraphicsCaptureSession as windows_core::Interface>::IID;
}
impl core::ops::Deref for GraphicsCaptureSession {
    type Target = IGraphicsCaptureSession;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for GraphicsCaptureSession {
    const NAME: &'static str = "Windows.Graphics.Capture.GraphicsCaptureSession";
}
unsafe impl Send for GraphicsCaptureSession {}
unsafe impl Sync for GraphicsCaptureSession {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Grid(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Grid, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Grid, Panel, FrameworkElement, UIElement, DependencyObject);
impl Grid {
    pub fn SetColumn<P0>(element: P0, value: i32) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FrameworkElement>,
    {
        Self::IGridStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetColumn)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                value,
            )
            .ok()
        })
    }
    pub fn SetColumnSpan<P0>(element: P0, value: i32) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FrameworkElement>,
    {
        Self::IGridStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetColumnSpan)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                value,
            )
            .ok()
        })
    }
    fn IGridStatics<R, F: FnOnce(&IGridStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Grid, IGridStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Grid {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IGrid>();
}
unsafe impl windows_core::Interface for Grid {
    type Vtable = <IGrid as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IGrid as windows_core::Interface>::IID;
}
impl core::ops::Deref for Grid {
    type Target = IGrid;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Grid {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Grid";
}
unsafe impl Send for Grid {}
unsafe impl Sync for Grid {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GridLength {
    pub value: f64,
    pub grid_unit_type: GridUnitType,
}
impl windows_core::imp::TypeKind for GridLength {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for GridLength {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.GridLength;f8;enum(Microsoft.UI.Xaml.GridUnitType;i4))",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GridUnitType(pub i32);
impl GridUnitType {
    pub const Auto: Self = Self(0);
    pub const Pixel: Self = Self(1);
    pub const Star: Self = Self(2);
}
impl windows_core::imp::TypeKind for GridUnitType {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for GridUnitType {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.GridUnitType;i4)");
}
pub type HANDLE = *mut core::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HARDWAREINPUT {
    pub uMsg: u32,
    pub wParamL: u16,
    pub wParamH: u16,
}
pub type HCERTSTORE = *mut core::ffi::c_void;
pub type HCRYPTPROV_LEGACY = usize;
pub type HINSTANCE = *mut core::ffi::c_void;
pub type HKEY = *mut core::ffi::c_void;
pub const HKEY_CURRENT_USER: HKEY = -2147483647 as _;
pub type HLOCAL = HANDLE;
pub type HMODULE = HINSTANCE;
pub type HWND = *mut core::ffi::c_void;
windows_core::imp::define_interface!(
    IAppActivationArguments,
    IAppActivationArguments_Vtbl,
    0x14f99eaf_1580_5062_bdc8_d5d1c31138fb
);
impl windows_core::RuntimeType for IAppActivationArguments {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppActivationArguments {
    pub fn Kind(&self) -> windows_core::Result<ExtendedActivationKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Kind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Data(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Data)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IAppActivationArguments_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Kind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut ExtendedActivationKind,
    ) -> windows_core::HRESULT,
    pub Data: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppInstance,
    IAppInstance_Vtbl,
    0x75766ae4_0239_5a26_b9da_d5bfc75a4866
);
impl windows_core::RuntimeType for IAppInstance {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppInstance {
    pub fn UnregisterKey(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).UnregisterKey)(windows_core::Interface::as_raw(
                self,
            ))
            .ok()
        }
    }
    pub fn RedirectActivationToAsync<P0>(
        &self,
        args: P0,
    ) -> windows_core::Result<windows_future::IAsyncAction>
    where
        P0: windows_core::Param<AppActivationArguments>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RedirectActivationToAsync)(
                windows_core::Interface::as_raw(self),
                args.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetActivatedEventArgs(&self) -> windows_core::Result<AppActivationArguments> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetActivatedEventArgs)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Activated<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<AppActivationArguments>,
            ) + 'static,
    {
        let handler: EventHandler<AppActivationArguments> = {
            let com =
                windows_core::imp::DelegateBox::<EventHandler<AppActivationArguments>, F>::new(
                    &EventHandlerBox::<AppActivationArguments, F>::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Activated)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveActivated,
            ))
        }
    }
    pub fn IsCurrent(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsCurrent)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ProcessId(&self) -> windows_core::Result<u32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ProcessId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IAppInstance_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub UnregisterKey: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub RedirectActivationToAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetActivatedEventArgs: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Activated: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveActivated:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Key: usize,
    pub IsCurrent:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub ProcessId:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppInstanceStatics,
    IAppInstanceStatics_Vtbl,
    0x4f414b25_8330_5a9b_bbc1_8229d479649d
);
impl windows_core::RuntimeType for IAppInstanceStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IAppInstanceStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetCurrent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    GetInstances: usize,
    pub FindOrRegisterForKey: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindow,
    IAppWindow_Vtbl,
    0xcfa788b3_643b_5c5e_ad4e_321d48a82acd
);
impl windows_core::RuntimeType for IAppWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindow {
    pub fn Id(&self) -> windows_core::Result<WindowId> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Id)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Position(&self) -> windows_core::Result<PointInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Position)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Presenter(&self) -> windows_core::Result<AppWindowPresenter> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Presenter)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Size(&self) -> windows_core::Result<SizeInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Size)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn TitleBar(&self) -> windows_core::Result<AppWindowTitleBar> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TitleBar)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn MoveAndResize(&self, rect: RectInt32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).MoveAndResize)(
                windows_core::Interface::as_raw(self),
                rect,
            )
            .ok()
        }
    }
    pub fn Resize(&self, size: SizeInt32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Resize)(
                windows_core::Interface::as_raw(self),
                size,
            )
            .ok()
        }
    }
    pub fn SetPresenterByKind(
        &self,
        appwindowpresenterkind: AppWindowPresenterKind,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPresenterByKind)(
                windows_core::Interface::as_raw(self),
                appwindowpresenterkind,
            )
            .ok()
        }
    }
    pub fn ShowWithActivation(&self, activatewindow: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).ShowWithActivation)(
                windows_core::Interface::as_raw(self),
                activatewindow,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IAppWindow_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Id:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut WindowId) -> windows_core::HRESULT,
    IsShownInSwitchers: usize,
    SetIsShownInSwitchers: usize,
    IsVisible: usize,
    OwnerWindowId: usize,
    pub Position:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut PointInt32) -> windows_core::HRESULT,
    pub Presenter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Size:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut SizeInt32) -> windows_core::HRESULT,
    Title: usize,
    SetTitle: usize,
    pub TitleBar: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Destroy: usize,
    Hide: usize,
    Move: usize,
    pub MoveAndResize:
        unsafe extern "system" fn(*mut core::ffi::c_void, RectInt32) -> windows_core::HRESULT,
    MoveAndResizeRelativeToDisplayArea: usize,
    pub Resize:
        unsafe extern "system" fn(*mut core::ffi::c_void, SizeInt32) -> windows_core::HRESULT,
    SetIcon: usize,
    SetIconWithIconId: usize,
    SetPresenter: usize,
    pub SetPresenterByKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        AppWindowPresenterKind,
    ) -> windows_core::HRESULT,
    Show: usize,
    pub ShowWithActivation:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindow2,
    IAppWindow2_Vtbl,
    0x6cd41292_794c_5cac_8961_210d012c6ebc
);
impl windows_core::RuntimeType for IAppWindow2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindow2 {
    pub fn MoveInZOrderAtBottom(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).MoveInZOrderAtBottom)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IAppWindow2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ClientSize: usize,
    pub MoveInZOrderAtBottom:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindowPresenter,
    IAppWindowPresenter_Vtbl,
    0xbc3042c2_c6c6_5632_8989_ff0ec6d3b40d
);
impl windows_core::RuntimeType for IAppWindowPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IAppWindowPresenter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IAppWindowTitleBar,
    IAppWindowTitleBar_Vtbl,
    0x5574efa2_c91c_5700_a363_539c71a7aaf4
);
impl windows_core::RuntimeType for IAppWindowTitleBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindowTitleBar {
    pub fn RightInset(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RightInset)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IAppWindowTitleBar_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    BackgroundColor: usize,
    SetBackgroundColor: usize,
    ButtonBackgroundColor: usize,
    SetButtonBackgroundColor: usize,
    ButtonForegroundColor: usize,
    SetButtonForegroundColor: usize,
    ButtonHoverBackgroundColor: usize,
    SetButtonHoverBackgroundColor: usize,
    ButtonHoverForegroundColor: usize,
    SetButtonHoverForegroundColor: usize,
    ButtonInactiveBackgroundColor: usize,
    SetButtonInactiveBackgroundColor: usize,
    ButtonInactiveForegroundColor: usize,
    SetButtonInactiveForegroundColor: usize,
    ButtonPressedBackgroundColor: usize,
    SetButtonPressedBackgroundColor: usize,
    ButtonPressedForegroundColor: usize,
    SetButtonPressedForegroundColor: usize,
    ExtendsContentIntoTitleBar: usize,
    SetExtendsContentIntoTitleBar: usize,
    ForegroundColor: usize,
    SetForegroundColor: usize,
    Height: usize,
    IconShowOptions: usize,
    SetIconShowOptions: usize,
    InactiveBackgroundColor: usize,
    SetInactiveBackgroundColor: usize,
    InactiveForegroundColor: usize,
    SetInactiveForegroundColor: usize,
    LeftInset: usize,
    pub RightInset:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAppWindowTitleBar3,
    IAppWindowTitleBar3_Vtbl,
    0x07146e74_0410_5597_aba7_1af276d2ae07
);
impl windows_core::RuntimeType for IAppWindowTitleBar3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAppWindowTitleBar3 {
    pub fn SetPreferredTheme(&self, value: TitleBarTheme) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredTheme)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IAppWindowTitleBar3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PreferredTheme: usize,
    pub SetPreferredTheme:
        unsafe extern "system" fn(*mut core::ffi::c_void, TitleBarTheme) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplication,
    IApplication_Vtbl,
    0x06a8f4e7_1146_55af_820d_ebd55643b021
);
impl windows_core::RuntimeType for IApplication {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IApplication {
    pub fn Resources(&self) -> windows_core::Result<ResourceDictionary> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Resources)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Exit(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Exit)(windows_core::Interface::as_raw(self)).ok()
        }
    }
}
#[repr(C)]
pub struct IApplication_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Resources: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SetResources: usize,
    DebugSettings: usize,
    RequestedTheme: usize,
    SetRequestedTheme: usize,
    FocusVisualKind: usize,
    SetFocusVisualKind: usize,
    HighContrastAdjustment: usize,
    SetHighContrastAdjustment: usize,
    UnhandledException: usize,
    RemoveUnhandledException: usize,
    pub Exit: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationFactory,
    IApplicationFactory_Vtbl,
    0x9fd96657_5294_5a65_a1db_4fea143597da
);
impl windows_core::RuntimeType for IApplicationFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationInitializationCallbackParams,
    IApplicationInitializationCallbackParams_Vtbl,
    0x1b1906ea_5b7b_5876_81ab_7c2281ac3d20
);
impl windows_core::RuntimeType for IApplicationInitializationCallbackParams {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationInitializationCallbackParams_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IApplicationOverrides,
    IApplicationOverrides_Vtbl,
    0xa33e81ef_c665_503b_8827_d27ef1720a06
);
impl windows_core::RuntimeType for IApplicationOverrides {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"Microsoft.UI.Xaml.IApplicationOverrides");
}
impl windows_core::RuntimeName for IApplicationOverrides {
    const NAME: &'static str = "Microsoft.UI.Xaml.IApplicationOverrides";
}
pub trait IApplicationOverrides_Impl: windows_core::IUnknownImpl {
    fn OnLaunched(
        &self,
        args: windows_core::Ref<LaunchActivatedEventArgs>,
    ) -> windows_core::Result<()>;
}
impl IApplicationOverrides_Vtbl {
    pub const fn new<Identity: IApplicationOverrides_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn OnLaunched<
            Identity: IApplicationOverrides_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            args: *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                IApplicationOverrides_Impl::OnLaunched(this, core::mem::transmute_copy(&args))
                    .into()
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<Identity, IApplicationOverrides, OFFSET>(
            ),
            OnLaunched: OnLaunched::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IApplicationOverrides as windows_core::Interface>::IID
    }
}
#[repr(C)]
pub struct IApplicationOverrides_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub OnLaunched: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IApplicationStatics,
    IApplicationStatics_Vtbl,
    0x4e0d09f5_4358_512c_a987_503b52848e95
);
impl windows_core::RuntimeType for IApplicationStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IApplicationStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Current: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Start: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAutoSuggestBox,
    IAutoSuggestBox_Vtbl,
    0x3eea809e_b2db_521d_97db_e0648fb5d798
);
impl windows_core::RuntimeType for IAutoSuggestBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAutoSuggestBox {
    pub fn IsSuggestionListOpen(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsSuggestionListOpen)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsSuggestionListOpen(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsSuggestionListOpen)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn TextChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<AutoSuggestBox>,
                windows_core::Ref<AutoSuggestBoxTextChangedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<AutoSuggestBox, AutoSuggestBoxTextChangedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < AutoSuggestBox , AutoSuggestBoxTextChangedEventArgs > , F >::new (& TypedEventHandlerBox::< AutoSuggestBox , AutoSuggestBoxTextChangedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TextChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTextChanged,
            ))
        }
    }
    pub fn QuerySubmitted<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<AutoSuggestBox>,
                windows_core::Ref<AutoSuggestBoxQuerySubmittedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<AutoSuggestBox, AutoSuggestBoxQuerySubmittedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < AutoSuggestBox , AutoSuggestBoxQuerySubmittedEventArgs > , F >::new (& TypedEventHandlerBox::< AutoSuggestBox , AutoSuggestBoxQuerySubmittedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).QuerySubmitted)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveQuerySubmitted,
            ))
        }
    }
}
#[repr(C)]
pub struct IAutoSuggestBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    MaxSuggestionListHeight: usize,
    SetMaxSuggestionListHeight: usize,
    pub IsSuggestionListOpen:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsSuggestionListOpen:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    TextMemberPath: usize,
    SetTextMemberPath: usize,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    UpdateTextOnSelect: usize,
    SetUpdateTextOnSelect: usize,
    PlaceholderText: usize,
    SetPlaceholderText: usize,
    Header: usize,
    SetHeader: usize,
    AutoMaximizeSuggestionArea: usize,
    SetAutoMaximizeSuggestionArea: usize,
    TextBoxStyle: usize,
    SetTextBoxStyle: usize,
    QueryIcon: usize,
    SetQueryIcon: usize,
    LightDismissOverlayMode: usize,
    SetLightDismissOverlayMode: usize,
    Description: usize,
    SetDescription: usize,
    SuggestionChosen: usize,
    RemoveSuggestionChosen: usize,
    pub TextChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTextChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub QuerySubmitted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveQuerySubmitted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAutoSuggestBoxQuerySubmittedEventArgs,
    IAutoSuggestBoxQuerySubmittedEventArgs_Vtbl,
    0x26da5de4_57a6_57bf_acc9_aac599c0b22b
);
impl windows_core::RuntimeType for IAutoSuggestBoxQuerySubmittedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAutoSuggestBoxQuerySubmittedEventArgs {
    pub fn QueryText(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).QueryText)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn ChosenSuggestion(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ChosenSuggestion)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IAutoSuggestBoxQuerySubmittedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub QueryText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ChosenSuggestion: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAutoSuggestBoxTextChangedEventArgs,
    IAutoSuggestBoxTextChangedEventArgs_Vtbl,
    0xd7191d84_e886_547f_a3e2_12f0e05b20fa
);
impl windows_core::RuntimeType for IAutoSuggestBoxTextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAutoSuggestBoxTextChangedEventArgs {
    pub fn Reason(&self) -> windows_core::Result<AutoSuggestionBoxTextChangeReason> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Reason)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IAutoSuggestBoxTextChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Reason: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut AutoSuggestionBoxTextChangeReason,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAutomationPeer,
    IAutomationPeer_Vtbl,
    0xe51d3e4e_34f0_568c_999f_6277e2afe6d7
);
impl windows_core::RuntimeType for IAutomationPeer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IAutomationPeer {
    pub fn GetPattern(
        &self,
        patterninterface: PatternInterface,
    ) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetPattern)(
                windows_core::Interface::as_raw(self),
                patterninterface,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IAutomationPeer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    EventsSource: usize,
    SetEventsSource: usize,
    pub GetPattern: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        PatternInterface,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IAutomationProperties,
    IAutomationProperties_Vtbl,
    0x525c6a71_dd8a_52a0_977b_db1b02f8e896
);
impl windows_core::RuntimeType for IAutomationProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IAutomationProperties_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IAutomationPropertiesStatics,
    IAutomationPropertiesStatics_Vtbl,
    0xb1e3e0f3_112f_5966_87dc_7862d4ad50e5
);
impl windows_core::RuntimeType for IAutomationPropertiesStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IAutomationPropertiesStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    AcceleratorKeyProperty: usize,
    GetAcceleratorKey: usize,
    SetAcceleratorKey: usize,
    AccessKeyProperty: usize,
    GetAccessKey: usize,
    SetAccessKey: usize,
    AutomationIdProperty: usize,
    GetAutomationId: usize,
    SetAutomationId: usize,
    HelpTextProperty: usize,
    GetHelpText: usize,
    SetHelpText: usize,
    IsRequiredForFormProperty: usize,
    GetIsRequiredForForm: usize,
    SetIsRequiredForForm: usize,
    ItemStatusProperty: usize,
    GetItemStatus: usize,
    SetItemStatus: usize,
    ItemTypeProperty: usize,
    GetItemType: usize,
    SetItemType: usize,
    LabeledByProperty: usize,
    GetLabeledBy: usize,
    SetLabeledBy: usize,
    NameProperty: usize,
    GetName: usize,
    pub SetName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBitmapEncoder,
    IBitmapEncoder_Vtbl,
    0x2bc468e3_e1f8_4b54_95e8_32919551ce62
);
impl windows_core::RuntimeType for IBitmapEncoder {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IBitmapEncoder {
    pub fn FlushAsync(&self) -> windows_core::Result<windows_future::IAsyncAction> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).FlushAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IBitmapEncoder_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    EncoderInformation: usize,
    BitmapProperties: usize,
    BitmapContainerProperties: usize,
    IsThumbnailGenerated: usize,
    SetIsThumbnailGenerated: usize,
    GeneratedThumbnailWidth: usize,
    SetGeneratedThumbnailWidth: usize,
    GeneratedThumbnailHeight: usize,
    SetGeneratedThumbnailHeight: usize,
    BitmapTransform: usize,
    SetPixelData: usize,
    GoToNextFrameAsync: usize,
    GoToNextFrameWithEncodingOptionsAsync: usize,
    pub FlushAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBitmapEncoderStatics,
    IBitmapEncoderStatics_Vtbl,
    0xa74356a7_a4e4_4eb9_8e40_564de7e1ccb2
);
impl windows_core::RuntimeType for IBitmapEncoderStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IBitmapEncoderStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    BmpEncoderId: usize,
    JpegEncoderId: usize,
    pub PngEncoderId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut windows_core::GUID,
    ) -> windows_core::HRESULT,
    TiffEncoderId: usize,
    GifEncoderId: usize,
    JpegXREncoderId: usize,
    GetEncoderInformationEnumerator: usize,
    pub CreateAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        windows_core::GUID,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBitmapEncoderWithSoftwareBitmap,
    IBitmapEncoderWithSoftwareBitmap_Vtbl,
    0x686cd241_4330_4c77_ace4_0334968b1768
);
impl windows_core::RuntimeType for IBitmapEncoderWithSoftwareBitmap {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IBitmapEncoderWithSoftwareBitmap {
    pub fn SetSoftwareBitmap<P0>(&self, bitmap: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<SoftwareBitmap>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSoftwareBitmap)(
                windows_core::Interface::as_raw(self),
                bitmap.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IBitmapEncoderWithSoftwareBitmap_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SetSoftwareBitmap: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBitmapImage,
    IBitmapImage_Vtbl,
    0x5cc29916_a411_5bc2_a3c5_a00d99a59da8
);
impl windows_core::RuntimeType for IBitmapImage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IBitmapImage {
    pub fn SetUriSource<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Uri>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetUriSource)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IBitmapImage_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    CreateOptions: usize,
    SetCreateOptions: usize,
    UriSource: usize,
    pub SetUriSource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBitmapSource,
    IBitmapSource_Vtbl,
    0x8424269d_9b82_534f_8fea_af5b5ef96bf2
);
impl windows_core::RuntimeType for IBitmapSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IBitmapSource {
    pub fn SetSourceAsync<P0>(
        &self,
        streamsource: P0,
    ) -> windows_core::Result<windows_future::IAsyncAction>
    where
        P0: windows_core::Param<IRandomAccessStream>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SetSourceAsync)(
                windows_core::Interface::as_raw(self),
                streamsource.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IBitmapSource_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PixelWidth: usize,
    PixelHeight: usize,
    SetSource: usize,
    pub SetSourceAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IBuffer,
    IBuffer_Vtbl,
    0x905a0fe0_bc53_11df_8c49_001e4fc686da
);
impl windows_core::RuntimeType for IBuffer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IBuffer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IBuffer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IBufferFactory,
    IBufferFactory_Vtbl,
    0x71af914d_c10f_484b_bc50_14bc623b3a27
);
impl windows_core::RuntimeType for IBufferFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IBufferFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Create: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IButton,
    IButton_Vtbl,
    0x216c183d_d07a_5aa5_b8a4_0300a2683e87
);
impl windows_core::RuntimeType for IButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IButton {
    pub fn Flyout(&self) -> windows_core::Result<FlyoutBase> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Flyout)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetFlyout)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IButton_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Flyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IButtonBase,
    IButtonBase_Vtbl,
    0x65714269_2473_5327_a652_0ea6bce7f403
);
impl windows_core::RuntimeType for IButtonBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IButtonBase {
    pub fn Click<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Click)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClick,
            ))
        }
    }
}
#[repr(C)]
pub struct IButtonBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ClickMode: usize,
    SetClickMode: usize,
    IsPointerOver: usize,
    IsPressed: usize,
    Command: usize,
    SetCommand: usize,
    CommandParameter: usize,
    SetCommandParameter: usize,
    pub Click: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClick:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
pub const ICON_BIG: i32 = 1;
pub const ICON_SMALL: i32 = 0;
windows_core::imp::define_interface!(
    IClipboardStatics,
    IClipboardStatics_Vtbl,
    0xc627e291_34e2_4963_8eed_93cbb0ea3d70
);
impl windows_core::RuntimeType for IClipboardStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IClipboardStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Flush: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IClosable,
    IClosable_Vtbl,
    0x30d5a829_7fa4_4026_83bb_d75bae4ea99e
);
impl windows_core::RuntimeType for IClosable {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IClosable,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IClosable {
    pub fn Close(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Close)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IClosable_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Close: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IColumnDefinition,
    IColumnDefinition_Vtbl,
    0x454cea14_87ec_5890_bb62_f1d82a94758e
);
impl windows_core::RuntimeType for IColumnDefinition {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IColumnDefinition {
    pub fn SetWidth(&self, value: GridLength) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IColumnDefinition_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Width: usize,
    pub SetWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, GridLength) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IComboBox,
    IComboBox_Vtbl,
    0xc77da58b_4fd7_51e0_a431_f84658a83e9e
);
impl windows_core::RuntimeType for IComboBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IComboBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContainerContentChangingEventArgs,
    IContainerContentChangingEventArgs_Vtbl,
    0xf4c8c937_b070_53ce_a76c_074ee5750a71
);
impl windows_core::RuntimeType for IContainerContentChangingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IContainerContentChangingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IContentControl,
    IContentControl_Vtbl,
    0x07e81761_11b2_52ae_8f8b_4d53d2b5900a
);
impl windows_core::RuntimeType for IContentControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IContentControl {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IContentControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IContentDialog,
    IContentDialog_Vtbl,
    0xac2145a3_4a32_5305_a81d_47509515bfce
);
impl windows_core::RuntimeType for IContentDialog {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IContentDialog {
    pub fn Hide(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Hide)(windows_core::Interface::as_raw(self)).ok()
        }
    }
    pub fn ShowAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<ContentDialogResult>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ShowAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IContentDialog_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Title: usize,
    SetTitle: usize,
    TitleTemplate: usize,
    SetTitleTemplate: usize,
    FullSizeDesired: usize,
    SetFullSizeDesired: usize,
    PrimaryButtonText: usize,
    SetPrimaryButtonText: usize,
    SecondaryButtonText: usize,
    SetSecondaryButtonText: usize,
    CloseButtonText: usize,
    SetCloseButtonText: usize,
    PrimaryButtonCommand: usize,
    SetPrimaryButtonCommand: usize,
    SecondaryButtonCommand: usize,
    SetSecondaryButtonCommand: usize,
    CloseButtonCommand: usize,
    SetCloseButtonCommand: usize,
    PrimaryButtonCommandParameter: usize,
    SetPrimaryButtonCommandParameter: usize,
    SecondaryButtonCommandParameter: usize,
    SetSecondaryButtonCommandParameter: usize,
    CloseButtonCommandParameter: usize,
    SetCloseButtonCommandParameter: usize,
    IsPrimaryButtonEnabled: usize,
    SetIsPrimaryButtonEnabled: usize,
    IsSecondaryButtonEnabled: usize,
    SetIsSecondaryButtonEnabled: usize,
    PrimaryButtonStyle: usize,
    SetPrimaryButtonStyle: usize,
    SecondaryButtonStyle: usize,
    SetSecondaryButtonStyle: usize,
    CloseButtonStyle: usize,
    SetCloseButtonStyle: usize,
    DefaultButton: usize,
    SetDefaultButton: usize,
    Closing: usize,
    RemoveClosing: usize,
    Closed: usize,
    RemoveClosed: usize,
    Opened: usize,
    RemoveOpened: usize,
    PrimaryButtonClick: usize,
    RemovePrimaryButtonClick: usize,
    SecondaryButtonClick: usize,
    RemoveSecondaryButtonClick: usize,
    CloseButtonClick: usize,
    RemoveCloseButtonClick: usize,
    pub Hide: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub ShowAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IControl,
    IControl_Vtbl,
    0x857d6e8a_d45a_5c69_a99c_bf6a5c54fb38
);
impl windows_core::RuntimeType for IControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IControl {
    pub fn IsEnabled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsEnabled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsFocusEngagementEnabled: usize,
    SetIsFocusEngagementEnabled: usize,
    IsFocusEngaged: usize,
    SetIsFocusEngaged: usize,
    RequiresPointer: usize,
    SetRequiresPointer: usize,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    IsTextScaleFactorEnabled: usize,
    SetIsTextScaleFactorEnabled: usize,
    pub IsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2,
    ICoreWebView2_Vtbl,
    0x3a3f559a_e5e9_5338_bb67_4eb0504a4f14
);
impl windows_core::RuntimeType for ICoreWebView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2 {
    pub fn Settings(&self) -> windows_core::Result<CoreWebView2Settings> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Settings)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Source(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Source)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn CanGoBack(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CanGoBack)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CanGoForward(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CanGoForward)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn DocumentTitle(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DocumentTitle)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn ContainsFullScreenElement(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContainsFullScreenElement)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn NavigationStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2NavigationStartingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2NavigationStartingEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2NavigationStartingEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2NavigationStartingEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).NavigationStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveNavigationStarting,
            ))
        }
    }
    pub fn ContentLoading<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2ContentLoadingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2ContentLoadingEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2ContentLoadingEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2ContentLoadingEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ContentLoading)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveContentLoading,
            ))
        }
    }
    pub fn SourceChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2SourceChangedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2SourceChangedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2SourceChangedEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2SourceChangedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SourceChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSourceChanged,
            ))
        }
    }
    pub fn HistoryChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).HistoryChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveHistoryChanged,
            ))
        }
    }
    pub fn NavigationCompleted<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2NavigationCompletedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2NavigationCompletedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2, CoreWebView2NavigationCompletedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2,
                        CoreWebView2NavigationCompletedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).NavigationCompleted)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveNavigationCompleted,
            ))
        }
    }
    pub fn PermissionRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2PermissionRequestedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2PermissionRequestedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2, CoreWebView2PermissionRequestedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2,
                        CoreWebView2PermissionRequestedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PermissionRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePermissionRequested,
            ))
        }
    }
    pub fn ProcessFailed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2ProcessFailedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2ProcessFailedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2ProcessFailedEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2ProcessFailedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ProcessFailed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveProcessFailed,
            ))
        }
    }
    pub fn WebMessageReceived<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2WebMessageReceivedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2WebMessageReceivedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2WebMessageReceivedEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2WebMessageReceivedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).WebMessageReceived)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveWebMessageReceived,
            ))
        }
    }
    pub fn NewWindowRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2NewWindowRequestedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2NewWindowRequestedEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2NewWindowRequestedEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2NewWindowRequestedEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).NewWindowRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveNewWindowRequested,
            ))
        }
    }
    pub fn DocumentTitleChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DocumentTitleChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDocumentTitleChanged,
            ))
        }
    }
    pub fn ContainsFullScreenElementChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ContainsFullScreenElementChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveContainsFullScreenElementChanged,
            ))
        }
    }
    pub fn WebResourceRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2WebResourceRequestedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2WebResourceRequestedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2, CoreWebView2WebResourceRequestedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2,
                        CoreWebView2WebResourceRequestedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).WebResourceRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveWebResourceRequested,
            ))
        }
    }
    pub fn WindowCloseRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).WindowCloseRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveWindowCloseRequested,
            ))
        }
    }
    pub fn Navigate(&self, uri: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Navigate)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(uri)),
            )
            .ok()
        }
    }
    pub fn NavigateToString(&self, htmlcontent: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).NavigateToString)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(htmlcontent)),
            )
            .ok()
        }
    }
    pub fn AddScriptToExecuteOnDocumentCreatedAsync(
        &self,
        javascript: &str,
    ) -> windows_core::Result<windows_future::IAsyncOperation<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AddScriptToExecuteOnDocumentCreatedAsync)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(javascript)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn ExecuteScriptAsync(
        &self,
        javascript: &str,
    ) -> windows_core::Result<windows_future::IAsyncOperation<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ExecuteScriptAsync)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(javascript)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn CapturePreviewAsync<P1>(
        &self,
        imageformat: CoreWebView2CapturePreviewImageFormat,
        imagestream: P1,
    ) -> windows_core::Result<windows_future::IAsyncAction>
    where
        P1: windows_core::Param<IRandomAccessStream>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CapturePreviewAsync)(
                windows_core::Interface::as_raw(self),
                imageformat,
                imagestream.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Reload(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Reload)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn CallDevToolsProtocolMethodAsync(
        &self,
        methodname: &str,
        parametersasjson: &str,
    ) -> windows_core::Result<windows_future::IAsyncOperation<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CallDevToolsProtocolMethodAsync)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(methodname)),
                core::mem::transmute_copy(&windows_core::HSTRING::from(parametersasjson)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GoBack(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).GoBack)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn GoForward(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).GoForward)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn GetDevToolsProtocolEventReceiver(
        &self,
        eventname: &str,
    ) -> windows_core::Result<CoreWebView2DevToolsProtocolEventReceiver> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDevToolsProtocolEventReceiver)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(eventname)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Stop(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Stop)(windows_core::Interface::as_raw(self)).ok()
        }
    }
    pub fn OpenDevToolsWindow(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).OpenDevToolsWindow)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Settings: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Source: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    BrowserProcessId: usize,
    pub CanGoBack:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub CanGoForward:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub DocumentTitle: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ContainsFullScreenElement:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub NavigationStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveNavigationStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ContentLoading: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveContentLoading:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub SourceChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSourceChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub HistoryChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveHistoryChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub NavigationCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveNavigationCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    FrameNavigationStarting: usize,
    RemoveFrameNavigationStarting: usize,
    FrameNavigationCompleted: usize,
    RemoveFrameNavigationCompleted: usize,
    ScriptDialogOpening: usize,
    RemoveScriptDialogOpening: usize,
    pub PermissionRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePermissionRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ProcessFailed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveProcessFailed:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub WebMessageReceived: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveWebMessageReceived:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub NewWindowRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveNewWindowRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub DocumentTitleChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDocumentTitleChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ContainsFullScreenElementChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveContainsFullScreenElementChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub WebResourceRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveWebResourceRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub WindowCloseRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveWindowCloseRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub Navigate: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub NavigateToString: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub AddScriptToExecuteOnDocumentCreatedAsync:
        unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
            *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT,
    RemoveScriptToExecuteOnDocumentCreated: usize,
    pub ExecuteScriptAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CapturePreviewAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2CapturePreviewImageFormat,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Reload: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    PostWebMessageAsJson: usize,
    PostWebMessageAsString: usize,
    pub CallDevToolsProtocolMethodAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GoBack: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub GoForward: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub GetDevToolsProtocolEventReceiver: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Stop: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    AddHostObjectToScript: usize,
    RemoveHostObjectFromScript: usize,
    pub OpenDevToolsWindow:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2BrowserExtension,
    ICoreWebView2BrowserExtension_Vtbl,
    0xbf991443_ee4f_57b8_bf2c_81cd6dbe1153
);
impl windows_core::RuntimeType for ICoreWebView2BrowserExtension {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2BrowserExtension {
    pub fn Id(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Id)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn Name(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Name)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn IsEnabled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsEnabled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn RemoveAsync(&self) -> windows_core::Result<windows_future::IAsyncAction> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RemoveAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn EnableAsync(
        &self,
        isenabled: bool,
    ) -> windows_core::Result<windows_future::IAsyncAction> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).EnableAsync)(
                windows_core::Interface::as_raw(self),
                isenabled,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2BrowserExtension_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Id: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Name: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub RemoveAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub EnableAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        bool,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2ContentLoadingEventArgs,
    ICoreWebView2ContentLoadingEventArgs_Vtbl,
    0x6cf95373_946c_5dae_9b3e_0fe23d5aa29f
);
impl windows_core::RuntimeType for ICoreWebView2ContentLoadingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICoreWebView2ContentLoadingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICoreWebView2ContextMenuItem,
    ICoreWebView2ContextMenuItem_Vtbl,
    0x2a65706f_941a_52cd_8651_a165586b0abf
);
impl windows_core::RuntimeType for ICoreWebView2ContextMenuItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2ContextMenuItem {
    pub fn Name(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Name)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn Label(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Label)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn CommandId(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CommandId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CustomItemSelected<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2ContextMenuItem>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2ContextMenuItem, windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2ContextMenuItem, windows_core::IInspectable>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2ContextMenuItem,
                        windows_core::IInspectable,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).CustomItemSelected)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveCustomItemSelected,
            ))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2ContextMenuItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Name: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Label: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CommandId:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    ShortcutKeyDescription: usize,
    Icon: usize,
    Kind: usize,
    IsEnabled: usize,
    SetIsEnabled: usize,
    IsChecked: usize,
    SetIsChecked: usize,
    Children: usize,
    pub CustomItemSelected: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveCustomItemSelected:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2ContextMenuRequestedEventArgs,
    ICoreWebView2ContextMenuRequestedEventArgs_Vtbl,
    0xd77bdd8c_9b3e_596e_ae80_320c0df4ecbc
);
impl windows_core::RuntimeType for ICoreWebView2ContextMenuRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2ContextMenuRequestedEventArgs {
    pub fn MenuItems(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<CoreWebView2ContextMenuItem>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).MenuItems)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn ContextMenuTarget(&self) -> windows_core::Result<CoreWebView2ContextMenuTarget> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContextMenuTarget)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetSelectedCommandId(&self, value: i32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedCommandId)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2ContextMenuRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub MenuItems: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ContextMenuTarget: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Location: usize,
    SelectedCommandId: usize,
    pub SetSelectedCommandId:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2ContextMenuTarget,
    ICoreWebView2ContextMenuTarget_Vtbl,
    0x41e24e6a_4612_5bd9_8e61_e9280615205e
);
impl windows_core::RuntimeType for ICoreWebView2ContextMenuTarget {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2ContextMenuTarget {
    pub fn Kind(&self) -> windows_core::Result<CoreWebView2ContextMenuTargetKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Kind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsEditable(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsEditable)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn HasLinkUri(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).HasLinkUri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn HasSelection(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).HasSelection)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SelectionText(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectionText)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2ContextMenuTarget_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Kind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2ContextMenuTargetKind,
    ) -> windows_core::HRESULT,
    pub IsEditable:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    IsRequestedForMainFrame: usize,
    PageUri: usize,
    FrameUri: usize,
    pub HasLinkUri:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    LinkUri: usize,
    HasLinkText: usize,
    LinkText: usize,
    HasSourceUri: usize,
    SourceUri: usize,
    pub HasSelection:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SelectionText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs,
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs_Vtbl,
    0xb6a4b41d_fd18_59fa_923a_c57555d960ce
);
impl windows_core::RuntimeType for ICoreWebView2DevToolsProtocolEventReceivedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2DevToolsProtocolEventReceivedEventArgs {
    pub fn ParameterObjectAsJson(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ParameterObjectAsJson)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2DevToolsProtocolEventReceivedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ParameterObjectAsJson: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs2,
    ICoreWebView2DevToolsProtocolEventReceivedEventArgs2_Vtbl,
    0x221728ba_635e_50d2_bd3c_fd22f4113978
);
impl windows_core::RuntimeType for ICoreWebView2DevToolsProtocolEventReceivedEventArgs2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2DevToolsProtocolEventReceivedEventArgs2 {
    pub fn SessionId(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SessionId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2DevToolsProtocolEventReceivedEventArgs2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SessionId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2DevToolsProtocolEventReceiver,
    ICoreWebView2DevToolsProtocolEventReceiver_Vtbl,
    0xb2a2be79_65fc_5537_8715_3d92bf31090b
);
impl windows_core::RuntimeType for ICoreWebView2DevToolsProtocolEventReceiver {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2DevToolsProtocolEventReceiver {
    pub fn DevToolsProtocolEventReceived<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2DevToolsProtocolEventReceivedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<
            CoreWebView2,
            CoreWebView2DevToolsProtocolEventReceivedEventArgs,
        > = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, CoreWebView2DevToolsProtocolEventReceivedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<
                    CoreWebView2,
                    CoreWebView2DevToolsProtocolEventReceivedEventArgs,
                    F,
                >::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DevToolsProtocolEventReceived)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDevToolsProtocolEventReceived,
            ))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2DevToolsProtocolEventReceiver_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub DevToolsProtocolEventReceived: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDevToolsProtocolEventReceived:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2DownloadOperation,
    ICoreWebView2DownloadOperation_Vtbl,
    0xafe73e6b_e760_5a06_9bf6_1e743c13cd2d
);
impl windows_core::RuntimeType for ICoreWebView2DownloadOperation {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2DownloadOperation {
    pub fn Uri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Uri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn TotalBytesToReceive(&self) -> windows_core::Result<i64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TotalBytesToReceive)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn BytesReceived(&self) -> windows_core::Result<i64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).BytesReceived)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn State(&self) -> windows_core::Result<CoreWebView2DownloadState> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).State)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn InterruptReason(&self) -> windows_core::Result<CoreWebView2DownloadInterruptReason> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).InterruptReason)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn BytesReceivedChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2DownloadOperation>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2DownloadOperation, windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2DownloadOperation, windows_core::IInspectable>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2DownloadOperation,
                        windows_core::IInspectable,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).BytesReceivedChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveBytesReceivedChanged,
            ))
        }
    }
    pub fn StateChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2DownloadOperation>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2DownloadOperation, windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2DownloadOperation, windows_core::IInspectable>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2DownloadOperation,
                        windows_core::IInspectable,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).StateChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveStateChanged,
            ))
        }
    }
    pub fn Cancel(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Cancel)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2DownloadOperation_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Uri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ContentDisposition: usize,
    MimeType: usize,
    pub TotalBytesToReceive:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i64) -> windows_core::HRESULT,
    pub BytesReceived:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i64) -> windows_core::HRESULT,
    EstimatedEndTime: usize,
    ResultFilePath: usize,
    pub State: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2DownloadState,
    ) -> windows_core::HRESULT,
    pub InterruptReason: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2DownloadInterruptReason,
    ) -> windows_core::HRESULT,
    CanResume: usize,
    pub BytesReceivedChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveBytesReceivedChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    EstimatedEndTimeChanged: usize,
    RemoveEstimatedEndTimeChanged: usize,
    pub StateChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveStateChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub Cancel: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2DownloadStartingEventArgs,
    ICoreWebView2DownloadStartingEventArgs_Vtbl,
    0x45d982ba_9256_5b35_b023_26a438599110
);
impl windows_core::RuntimeType for ICoreWebView2DownloadStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2DownloadStartingEventArgs {
    pub fn DownloadOperation(&self) -> windows_core::Result<CoreWebView2DownloadOperation> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DownloadOperation)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn ResultFilePath(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ResultFilePath)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetResultFilePath(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetResultFilePath)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetDeferral(&self) -> windows_core::Result<Deferral> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDeferral)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2DownloadStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub DownloadOperation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Cancel: usize,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub ResultFilePath: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetResultFilePath: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub GetDeferral: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Environment,
    ICoreWebView2Environment_Vtbl,
    0xd8cc7831_b783_556b_b9ce_899c1e95d585
);
impl windows_core::RuntimeType for ICoreWebView2Environment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Environment {
    pub fn BrowserVersionString(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).BrowserVersionString)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn CreateWebResourceResponse<P0>(
        &self,
        content: P0,
        statuscode: i32,
        reasonphrase: &str,
        headers: &str,
    ) -> windows_core::Result<CoreWebView2WebResourceResponse>
    where
        P0: windows_core::Param<IRandomAccessStream>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateWebResourceResponse)(
                windows_core::Interface::as_raw(self),
                content.param().abi(),
                statuscode,
                core::mem::transmute_copy(&windows_core::HSTRING::from(reasonphrase)),
                core::mem::transmute_copy(&windows_core::HSTRING::from(headers)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Environment_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub BrowserVersionString: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    NewBrowserVersionAvailable: usize,
    RemoveNewBrowserVersionAvailable: usize,
    CreateCoreWebView2ControllerAsync: usize,
    pub CreateWebResourceResponse: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        i32,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Environment15,
    ICoreWebView2Environment15_Vtbl,
    0x37b49c50_b262_5563_a5ed_ae60182495c0
);
impl windows_core::RuntimeType for ICoreWebView2Environment15 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Environment15 {
    pub fn CreateFindOptions(&self) -> windows_core::Result<CoreWebView2FindOptions> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateFindOptions)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Environment15_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateFindOptions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Environment9,
    ICoreWebView2Environment9_Vtbl,
    0xc8213ec7_7dc9_5468_a88b_15c6b7144478
);
impl windows_core::RuntimeType for ICoreWebView2Environment9 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Environment9 {
    pub fn CreateContextMenuItem<P1>(
        &self,
        label: &str,
        iconstream: P1,
        kind: CoreWebView2ContextMenuItemKind,
    ) -> windows_core::Result<CoreWebView2ContextMenuItem>
    where
        P1: windows_core::Param<IRandomAccessStream>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateContextMenuItem)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(label)),
                iconstream.param().abi(),
                kind,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Environment9_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateContextMenuItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        CoreWebView2ContextMenuItemKind,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2EnvironmentOptions,
    ICoreWebView2EnvironmentOptions_Vtbl,
    0x25d6dc39_0062_5735_8b09_a6f535f19e97
);
impl windows_core::RuntimeType for ICoreWebView2EnvironmentOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2EnvironmentOptions {
    pub fn SetAdditionalBrowserArguments(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAdditionalBrowserArguments)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2EnvironmentOptions_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    AdditionalBrowserArguments: usize,
    pub SetAdditionalBrowserArguments: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2EnvironmentOptions6,
    ICoreWebView2EnvironmentOptions6_Vtbl,
    0xeb5b14c2_6f05_514e_b19a_76744d1ce684
);
impl windows_core::RuntimeType for ICoreWebView2EnvironmentOptions6 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2EnvironmentOptions6 {
    pub fn SetAreBrowserExtensionsEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAreBrowserExtensionsEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2EnvironmentOptions6_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    AreBrowserExtensionsEnabled: usize,
    pub SetAreBrowserExtensionsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2EnvironmentStatics,
    ICoreWebView2EnvironmentStatics_Vtbl,
    0x0e33f804_f20b_5635_8491_162aaa27517b
);
impl windows_core::RuntimeType for ICoreWebView2EnvironmentStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICoreWebView2EnvironmentStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    CreateAsync: usize,
    pub CreateWithOptionsAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetAvailableBrowserVersionString: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetAvailableBrowserVersionString2: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Find,
    ICoreWebView2Find_Vtbl,
    0xd9afbd37_aeb2_5109_bb8f_6ff27cf9d279
);
impl windows_core::RuntimeType for ICoreWebView2Find {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Find {
    pub fn MatchCount(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).MatchCount)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn StartAsync<P0>(&self, options: P0) -> windows_core::Result<windows_future::IAsyncAction>
    where
        P0: windows_core::Param<CoreWebView2FindOptions>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).StartAsync)(
                windows_core::Interface::as_raw(self),
                options.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Stop(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Stop)(windows_core::Interface::as_raw(self)).ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Find_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ActiveMatchIndex: usize,
    pub MatchCount:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    ActiveMatchIndexChanged: usize,
    RemoveActiveMatchIndexChanged: usize,
    MatchCountChanged: usize,
    RemoveMatchCountChanged: usize,
    pub StartAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    FindNext: usize,
    FindPrevious: usize,
    pub Stop: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2FindOptions,
    ICoreWebView2FindOptions_Vtbl,
    0x157b920b_e1dc_5792_8a21_26a1c882c3f6
);
impl windows_core::RuntimeType for ICoreWebView2FindOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2FindOptions {
    pub fn SetFindTerm(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetFindTerm)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetSuppressDefaultFindDialog(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSuppressDefaultFindDialog)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2FindOptions_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FindTerm: usize,
    pub SetFindTerm: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    IsCaseSensitive: usize,
    SetIsCaseSensitive: usize,
    ShouldHighlightAllMatches: usize,
    SetShouldHighlightAllMatches: usize,
    ShouldMatchWord: usize,
    SetShouldMatchWord: usize,
    SuppressDefaultFindDialog: usize,
    pub SetSuppressDefaultFindDialog:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2FrameInfo,
    ICoreWebView2FrameInfo_Vtbl,
    0xf9b82e06_73f3_513b_bc2c_445ddedba976
);
impl windows_core::RuntimeType for ICoreWebView2FrameInfo {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2FrameInfo {
    pub fn Source(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Source)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2FrameInfo_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Name: usize,
    pub Source: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2NavigationCompletedEventArgs,
    ICoreWebView2NavigationCompletedEventArgs_Vtbl,
    0x4865e238_036a_5664_95a3_447ec44cf498
);
impl windows_core::RuntimeType for ICoreWebView2NavigationCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2NavigationCompletedEventArgs {
    pub fn IsSuccess(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsSuccess)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn WebErrorStatus(&self) -> windows_core::Result<CoreWebView2WebErrorStatus> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).WebErrorStatus)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn NavigationId(&self) -> windows_core::Result<u64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).NavigationId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2NavigationCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsSuccess:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub WebErrorStatus: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2WebErrorStatus,
    ) -> windows_core::HRESULT,
    pub NavigationId:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2NavigationStartingEventArgs,
    ICoreWebView2NavigationStartingEventArgs_Vtbl,
    0x548d23d3_fea3_5616_bd05_ae08066c86d3
);
impl windows_core::RuntimeType for ICoreWebView2NavigationStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2NavigationStartingEventArgs {
    pub fn Uri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Uri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn IsRedirected(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsRedirected)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn NavigationId(&self) -> windows_core::Result<u64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).NavigationId)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2NavigationStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Uri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    IsUserInitiated: usize,
    pub IsRedirected:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    RequestHeaders: usize,
    Cancel: usize,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub NavigationId:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2NewWindowRequestedEventArgs,
    ICoreWebView2NewWindowRequestedEventArgs_Vtbl,
    0xe6e013ba_aec8_532e_9ac9_1590af7b25ec
);
impl windows_core::RuntimeType for ICoreWebView2NewWindowRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2NewWindowRequestedEventArgs {
    pub fn Uri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Uri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetNewWindow<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<CoreWebView2>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetNewWindow)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn IsUserInitiated(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsUserInitiated)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn GetDeferral(&self) -> windows_core::Result<Deferral> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDeferral)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2NewWindowRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Uri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    NewWindow: usize,
    pub SetNewWindow: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub IsUserInitiated:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    WindowFeatures: usize,
    pub GetDeferral: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2PermissionRequestedEventArgs,
    ICoreWebView2PermissionRequestedEventArgs_Vtbl,
    0x118bdd9b_cef1_5910_929e_c1a321328239
);
impl windows_core::RuntimeType for ICoreWebView2PermissionRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2PermissionRequestedEventArgs {
    pub fn Uri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Uri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn PermissionKind(&self) -> windows_core::Result<CoreWebView2PermissionKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PermissionKind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsUserInitiated(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsUserInitiated)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn State(&self) -> windows_core::Result<CoreWebView2PermissionState> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).State)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetState(&self, value: CoreWebView2PermissionState) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetState)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetDeferral(&self) -> windows_core::Result<Deferral> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDeferral)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2PermissionRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Uri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PermissionKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2PermissionKind,
    ) -> windows_core::HRESULT,
    pub IsUserInitiated:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub State: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2PermissionState,
    ) -> windows_core::HRESULT,
    pub SetState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2PermissionState,
    ) -> windows_core::HRESULT,
    pub GetDeferral: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2PermissionRequestedEventArgs3,
    ICoreWebView2PermissionRequestedEventArgs3_Vtbl,
    0x200e8bcc_bc11_5beb_aa7a_79d4c95d73aa
);
impl windows_core::RuntimeType for ICoreWebView2PermissionRequestedEventArgs3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2PermissionRequestedEventArgs3 {
    pub fn SetSavesInProfile(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSavesInProfile)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2PermissionRequestedEventArgs3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    SavesInProfile: usize,
    pub SetSavesInProfile:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2PermissionSetting,
    ICoreWebView2PermissionSetting_Vtbl,
    0xb4158d0b_8ef8_575f_8e99_5fe02e8b579e
);
impl windows_core::RuntimeType for ICoreWebView2PermissionSetting {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2PermissionSetting {
    pub fn PermissionKind(&self) -> windows_core::Result<CoreWebView2PermissionKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PermissionKind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn PermissionOrigin(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PermissionOrigin)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn PermissionState(&self) -> windows_core::Result<CoreWebView2PermissionState> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PermissionState)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2PermissionSetting_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub PermissionKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2PermissionKind,
    ) -> windows_core::HRESULT,
    pub PermissionOrigin: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PermissionState: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2PermissionState,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2ProcessFailedEventArgs,
    ICoreWebView2ProcessFailedEventArgs_Vtbl,
    0x25a8f8c9_d944_539d_afa3_24172b48ef47
);
impl windows_core::RuntimeType for ICoreWebView2ProcessFailedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2ProcessFailedEventArgs {
    pub fn ProcessFailedKind(&self) -> windows_core::Result<CoreWebView2ProcessFailedKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ProcessFailedKind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2ProcessFailedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ProcessFailedKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut CoreWebView2ProcessFailedKind,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Profile,
    ICoreWebView2Profile_Vtbl,
    0xd4bdd25c_a2db_5c03_9659_abdeb9793621
);
impl windows_core::RuntimeType for ICoreWebView2Profile {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICoreWebView2Profile_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICoreWebView2Profile2,
    ICoreWebView2Profile2_Vtbl,
    0x93d21e18_1b06_59d0_9687_10f4844b016d
);
impl windows_core::RuntimeType for ICoreWebView2Profile2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Profile2 {
    pub fn ClearBrowsingDataAsync(
        &self,
        datakinds: CoreWebView2BrowsingDataKinds,
    ) -> windows_core::Result<windows_future::IAsyncAction> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ClearBrowsingDataAsync)(
                windows_core::Interface::as_raw(self),
                datakinds,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Profile2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ClearBrowsingDataAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2BrowsingDataKinds,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Profile3,
    ICoreWebView2Profile3_Vtbl,
    0x507ed587_c511_5e47_be5b_fc9ccdf179b6
);
impl windows_core::RuntimeType for ICoreWebView2Profile3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Profile3 {
    pub fn SetPreferredTrackingPreventionLevel(
        &self,
        value: CoreWebView2TrackingPreventionLevel,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredTrackingPreventionLevel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Profile3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PreferredTrackingPreventionLevel: usize,
    pub SetPreferredTrackingPreventionLevel: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2TrackingPreventionLevel,
    )
        -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Profile4,
    ICoreWebView2Profile4_Vtbl,
    0xeeae109a_f641_5a5b_942f_9922594ffb4d
);
impl windows_core::RuntimeType for ICoreWebView2Profile4 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Profile4 {
    pub fn SetPermissionStateAsync(
        &self,
        permissionkind: CoreWebView2PermissionKind,
        origin: &str,
        state: CoreWebView2PermissionState,
    ) -> windows_core::Result<windows_future::IAsyncAction> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SetPermissionStateAsync)(
                windows_core::Interface::as_raw(self),
                permissionkind,
                core::mem::transmute_copy(&windows_core::HSTRING::from(origin)),
                state,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Profile4_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SetPermissionStateAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2PermissionKind,
        *mut core::ffi::c_void,
        CoreWebView2PermissionState,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Profile7,
    ICoreWebView2Profile7_Vtbl,
    0x5f665761_5c12_5f39_b9fe_607e6e94add1
);
impl windows_core::RuntimeType for ICoreWebView2Profile7 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Profile7 {
    pub fn AddBrowserExtensionAsync(
        &self,
        extensionfolderpath: &str,
    ) -> windows_core::Result<windows_future::IAsyncOperation<CoreWebView2BrowserExtension>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AddBrowserExtensionAsync)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(extensionfolderpath)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Profile7_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub AddBrowserExtensionAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2SaveAsUIShowingEventArgs,
    ICoreWebView2SaveAsUIShowingEventArgs_Vtbl,
    0xcc39a250_2b4c_5608_9097_c59b8a8231b9
);
impl windows_core::RuntimeType for ICoreWebView2SaveAsUIShowingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2SaveAsUIShowingEventArgs {
    pub fn ContentMimeType(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContentMimeType)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetSuppressDefaultDialog(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSuppressDefaultDialog)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetSaveAsFilePath(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSaveAsFilePath)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetAllowReplace(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAllowReplace)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetKind(&self, value: CoreWebView2SaveAsKind) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetKind)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2SaveAsUIShowingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ContentMimeType: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Cancel: usize,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    SuppressDefaultDialog: usize,
    pub SetSuppressDefaultDialog:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    SaveAsFilePath: usize,
    pub SetSaveAsFilePath: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    AllowReplace: usize,
    pub SetAllowReplace:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Kind: usize,
    pub SetKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2SaveAsKind,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2ScreenCaptureStartingEventArgs,
    ICoreWebView2ScreenCaptureStartingEventArgs_Vtbl,
    0x35f0e2bb_94b0_5be7_b633_f87244e38bfe
);
impl windows_core::RuntimeType for ICoreWebView2ScreenCaptureStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2ScreenCaptureStartingEventArgs {
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn OriginalSourceFrameInfo(&self) -> windows_core::Result<CoreWebView2FrameInfo> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).OriginalSourceFrameInfo)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetDeferral(&self) -> windows_core::Result<Deferral> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetDeferral)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2ScreenCaptureStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Cancel: usize,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub OriginalSourceFrameInfo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetDeferral: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Settings,
    ICoreWebView2Settings_Vtbl,
    0x003b325e_74cd_52dd_8024_ebb8be38e48e
);
impl windows_core::RuntimeType for ICoreWebView2Settings {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Settings {
    pub fn SetIsWebMessageEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsWebMessageEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetAreDevToolsEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetAreDevToolsEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Settings_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsScriptEnabled: usize,
    SetIsScriptEnabled: usize,
    IsWebMessageEnabled: usize,
    pub SetIsWebMessageEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    AreDefaultScriptDialogsEnabled: usize,
    SetAreDefaultScriptDialogsEnabled: usize,
    IsStatusBarEnabled: usize,
    SetIsStatusBarEnabled: usize,
    AreDevToolsEnabled: usize,
    pub SetAreDevToolsEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2Settings4,
    ICoreWebView2Settings4_Vtbl,
    0xd6a955f0_daef_5a6a_a6f6_c72f0ede7620
);
impl windows_core::RuntimeType for ICoreWebView2Settings4 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2Settings4 {
    pub fn IsPasswordAutosaveEnabled(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsPasswordAutosaveEnabled)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsPasswordAutosaveEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsPasswordAutosaveEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsGeneralAutofillEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsGeneralAutofillEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2Settings4_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsPasswordAutosaveEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsPasswordAutosaveEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    IsGeneralAutofillEnabled: usize,
    pub SetIsGeneralAutofillEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2SourceChangedEventArgs,
    ICoreWebView2SourceChangedEventArgs_Vtbl,
    0xca437b2c_6a18_5552_b749_b198f8cc34d9
);
impl windows_core::RuntimeType for ICoreWebView2SourceChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2SourceChangedEventArgs {
    pub fn IsNewDocument(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsNewDocument)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2SourceChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsNewDocument:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2WebMessageReceivedEventArgs,
    ICoreWebView2WebMessageReceivedEventArgs_Vtbl,
    0xeb066159_b725_5d5b_adc8_f5d7b9290304
);
impl windows_core::RuntimeType for ICoreWebView2WebMessageReceivedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2WebMessageReceivedEventArgs {
    pub fn TryGetWebMessageAsString(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TryGetWebMessageAsString)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2WebMessageReceivedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Source: usize,
    WebMessageAsJson: usize,
    pub TryGetWebMessageAsString: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2WebResourceRequest,
    ICoreWebView2WebResourceRequest_Vtbl,
    0x5c742259_67d2_5df2_8382_0f201b4d7197
);
impl windows_core::RuntimeType for ICoreWebView2WebResourceRequest {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2WebResourceRequest {
    pub fn Uri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Uri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2WebResourceRequest_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Uri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2WebResourceRequestedEventArgs,
    ICoreWebView2WebResourceRequestedEventArgs_Vtbl,
    0x577f1fc4_c943_54a9_9700_bd469b48bd41
);
impl windows_core::RuntimeType for ICoreWebView2WebResourceRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2WebResourceRequestedEventArgs {
    pub fn Request(&self) -> windows_core::Result<CoreWebView2WebResourceRequest> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Request)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetResponse<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<CoreWebView2WebResourceResponse>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetResponse)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2WebResourceRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Request: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Response: usize,
    pub SetResponse: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2WebResourceResponse,
    ICoreWebView2WebResourceResponse_Vtbl,
    0x14621923_e485_5f44_8f5d_bd4243bc398f
);
impl windows_core::RuntimeType for ICoreWebView2WebResourceResponse {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICoreWebView2WebResourceResponse_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ICoreWebView2_11,
    ICoreWebView2_11_Vtbl,
    0xc00acbb1_ae32_501f_ad19_9d0ac32d6142
);
impl windows_core::RuntimeType for ICoreWebView2_11 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_11 {
    pub fn ContextMenuRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2ContextMenuRequestedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2ContextMenuRequestedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2, CoreWebView2ContextMenuRequestedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2,
                        CoreWebView2ContextMenuRequestedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ContextMenuRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveContextMenuRequested,
            ))
        }
    }
    pub fn CallDevToolsProtocolMethodForSessionAsync(
        &self,
        sessionid: &str,
        methodname: &str,
        parametersasjson: &str,
    ) -> windows_core::Result<windows_future::IAsyncOperation<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CallDevToolsProtocolMethodForSessionAsync)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(sessionid)),
                core::mem::transmute_copy(&windows_core::HSTRING::from(methodname)),
                core::mem::transmute_copy(&windows_core::HSTRING::from(parametersasjson)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_11_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ContextMenuRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveContextMenuRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub CallDevToolsProtocolMethodForSessionAsync:
        unsafe extern "system" fn(
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
            *mut core::ffi::c_void,
            *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_13,
    ICoreWebView2_13_Vtbl,
    0x314b5846_dbc7_5de4_a792_647ea0f3296a
);
impl windows_core::RuntimeType for ICoreWebView2_13 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_13 {
    pub fn Profile(&self) -> windows_core::Result<CoreWebView2Profile> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Profile)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_13_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Profile: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_15,
    ICoreWebView2_15_Vtbl,
    0x4443f532_d2ba_5ae2_a9b3_8de62bd5d4a9
);
impl windows_core::RuntimeType for ICoreWebView2_15 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_15 {
    pub fn FaviconUri(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).FaviconUri)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn FaviconChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).FaviconChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveFaviconChanged,
            ))
        }
    }
    pub fn GetFaviconAsync(
        &self,
        format: CoreWebView2FaviconImageFormat,
    ) -> windows_core::Result<windows_future::IAsyncOperation<IRandomAccessStream>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetFaviconAsync)(
                windows_core::Interface::as_raw(self),
                format,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_15_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub FaviconUri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub FaviconChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveFaviconChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub GetFaviconAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2FaviconImageFormat,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_16,
    ICoreWebView2_16_Vtbl,
    0x61d0a57c_6c4f_50ff_a137_314b0099a2b8
);
impl windows_core::RuntimeType for ICoreWebView2_16 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_16 {
    pub fn ShowPrintUI(
        &self,
        printdialogkind: CoreWebView2PrintDialogKind,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).ShowPrintUI)(
                windows_core::Interface::as_raw(self),
                printdialogkind,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_16_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PrintAsync: usize,
    pub ShowPrintUI: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        CoreWebView2PrintDialogKind,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_25,
    ICoreWebView2_25_Vtbl,
    0xb8e2edce_d943_5871_8397_483dbd6c0f9e
);
impl windows_core::RuntimeType for ICoreWebView2_25 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_25 {
    pub fn SaveAsUIShowing<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2SaveAsUIShowingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2SaveAsUIShowingEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2SaveAsUIShowingEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2SaveAsUIShowingEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SaveAsUIShowing)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSaveAsUIShowing,
            ))
        }
    }
    pub fn ShowSaveAsUIAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<CoreWebView2SaveAsUIResult>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ShowSaveAsUIAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_25_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SaveAsUIShowing: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSaveAsUIShowing:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ShowSaveAsUIAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_27,
    ICoreWebView2_27_Vtbl,
    0xd964f497_ffdf_5bcd_bf52_ff4585f2ebc2
);
impl windows_core::RuntimeType for ICoreWebView2_27 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_27 {
    pub fn ScreenCaptureStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2ScreenCaptureStartingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2ScreenCaptureStartingEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<CoreWebView2, CoreWebView2ScreenCaptureStartingEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        CoreWebView2,
                        CoreWebView2ScreenCaptureStartingEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ScreenCaptureStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveScreenCaptureStarting,
            ))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_27_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ScreenCaptureStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveScreenCaptureStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_28,
    ICoreWebView2_28_Vtbl,
    0x6e43a033_d2e9_59fc_8856_88d4f7438c4d
);
impl windows_core::RuntimeType for ICoreWebView2_28 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_28 {
    pub fn Find(&self) -> windows_core::Result<CoreWebView2Find> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Find)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_28_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Find: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_4,
    ICoreWebView2_4_Vtbl,
    0x4ac595ce_1502_5775_b2c8_22c11a369c25
);
impl windows_core::RuntimeType for ICoreWebView2_4 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_4 {
    pub fn DownloadStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<CoreWebView2>,
                windows_core::Ref<CoreWebView2DownloadStartingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, CoreWebView2DownloadStartingEventArgs> = {
            let com = windows_core::imp::DelegateBox::< TypedEventHandler < CoreWebView2 , CoreWebView2DownloadStartingEventArgs > , F >::new (& TypedEventHandlerBox::< CoreWebView2 , CoreWebView2DownloadStartingEventArgs , F >::VTABLE , handler) ;
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DownloadStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDownloadStarting,
            ))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_4_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FrameCreated: usize,
    RemoveFrameCreated: usize,
    pub DownloadStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDownloadStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_8,
    ICoreWebView2_8_Vtbl,
    0xaa2503c0_8d1c_5a3d_b898_f55f7595268a
);
impl windows_core::RuntimeType for ICoreWebView2_8 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_8 {
    pub fn IsMuted(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsMuted)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsMuted(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsMuted)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn IsDocumentPlayingAudio(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsDocumentPlayingAudio)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsMutedChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).IsMutedChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveIsMutedChanged,
            ))
        }
    }
    pub fn IsDocumentPlayingAudioChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).IsDocumentPlayingAudioChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveIsDocumentPlayingAudioChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_8_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsMuted:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsMuted:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub IsDocumentPlayingAudio:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub IsMutedChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveIsMutedChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub IsDocumentPlayingAudioChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveIsDocumentPlayingAudioChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_9,
    ICoreWebView2_9_Vtbl,
    0x64b2ec16_0b29_5216_bf86_e575c88f7031
);
impl windows_core::RuntimeType for ICoreWebView2_9 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_9 {
    pub fn IsDefaultDownloadDialogOpen(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsDefaultDownloadDialogOpen)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn IsDefaultDownloadDialogOpenChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<CoreWebView2>, windows_core::Ref<windows_core::IInspectable>)
            + 'static,
    {
        let handler: TypedEventHandler<CoreWebView2, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<CoreWebView2, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<CoreWebView2, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self)
                .IsDefaultDownloadDialogOpenChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveIsDefaultDownloadDialogOpenChanged,
            ))
        }
    }
    pub fn CloseDefaultDownloadDialog(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).CloseDefaultDownloadDialog)(
                windows_core::Interface::as_raw(self),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_9_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsDefaultDownloadDialogOpen:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    DefaultDownloadDialogCornerAlignment: usize,
    SetDefaultDownloadDialogCornerAlignment: usize,
    DefaultDownloadDialogMargin: usize,
    SetDefaultDownloadDialogMargin: usize,
    pub IsDefaultDownloadDialogOpenChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveIsDefaultDownloadDialogOpenChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    OpenDefaultDownloadDialog: usize,
    pub CloseDefaultDownloadDialog:
        unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICoreWebView2_Manual,
    ICoreWebView2_Manual_Vtbl,
    0x2d988546_9962_516b_be53_859fb0f50179
);
impl windows_core::RuntimeType for ICoreWebView2_Manual {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ICoreWebView2_Manual {
    pub fn AddWebResourceRequestedFilter(
        &self,
        uri: &str,
        resourcecontext: CoreWebView2WebResourceContext,
        requestsourcekinds: CoreWebView2WebResourceRequestSourceKinds,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).AddWebResourceRequestedFilter)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(uri)),
                resourcecontext,
                requestsourcekinds,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ICoreWebView2_Manual_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub AddWebResourceRequestedFilter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        CoreWebView2WebResourceContext,
        CoreWebView2WebResourceRequestSourceKinds,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ICryptographicBufferStatics,
    ICryptographicBufferStatics_Vtbl,
    0x320b7e22_3cb0_4cdf_8663_1d28910065eb
);
impl windows_core::RuntimeType for ICryptographicBufferStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ICryptographicBufferStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Compare: usize,
    GenerateRandom: usize,
    GenerateRandomNumber: usize,
    pub CreateFromByteArray: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *const u8,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ID3D11Device,
    ID3D11Device_Vtbl,
    0xdb6f6ddb_ac77_4e88_8253_819df9bbf140
);
windows_core::imp::interface_hierarchy!(ID3D11Device, windows_core::IUnknown);
#[repr(C)]
pub struct ID3D11Device_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    CreateBuffer: usize,
    CreateTexture1D: usize,
    CreateTexture2D: usize,
    CreateTexture3D: usize,
    CreateShaderResourceView: usize,
    CreateUnorderedAccessView: usize,
    CreateRenderTargetView: usize,
    CreateDepthStencilView: usize,
    CreateInputLayout: usize,
    CreateVertexShader: usize,
    CreateGeometryShader: usize,
    CreateGeometryShaderWithStreamOutput: usize,
    CreatePixelShader: usize,
    CreateHullShader: usize,
    CreateDomainShader: usize,
    CreateComputeShader: usize,
    CreateClassLinkage: usize,
    CreateBlendState: usize,
    CreateDepthStencilState: usize,
    CreateRasterizerState: usize,
    CreateSamplerState: usize,
    CreateQuery: usize,
    CreatePredicate: usize,
    CreateCounter: usize,
    CreateDeferredContext: usize,
    OpenSharedResource: usize,
    CheckFormatSupport: usize,
    CheckMultisampleQualityLevels: usize,
    CheckCounterInfo: usize,
    CheckCounter: usize,
    CheckFeatureSupport: usize,
    GetPrivateData: usize,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
    GetFeatureLevel: usize,
    GetCreationFlags: usize,
    GetDeviceRemovedReason: usize,
    GetImmediateContext: usize,
    SetExceptionMode: usize,
    GetExceptionMode: usize,
}
windows_core::imp::define_interface!(
    ID3D11DeviceChild,
    ID3D11DeviceChild_Vtbl,
    0x1841e5c8_16b0_489b_bcc8_44cfb0d5deae
);
windows_core::imp::interface_hierarchy!(ID3D11DeviceChild, windows_core::IUnknown);
#[repr(C)]
pub struct ID3D11DeviceChild_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    GetDevice: usize,
    GetPrivateData: usize,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
}
windows_core::imp::define_interface!(
    ID3D11DeviceContext,
    ID3D11DeviceContext_Vtbl,
    0xc0bfa96c_e089_44fb_8eaf_26f8796190da
);
impl core::ops::Deref for ID3D11DeviceContext {
    type Target = ID3D11DeviceChild;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(
    ID3D11DeviceContext,
    windows_core::IUnknown,
    ID3D11DeviceChild
);
#[repr(C)]
pub struct ID3D11DeviceContext_Vtbl {
    pub base__: ID3D11DeviceChild_Vtbl,
    VSSetConstantBuffers: usize,
    PSSetShaderResources: usize,
    PSSetShader: usize,
    PSSetSamplers: usize,
    VSSetShader: usize,
    DrawIndexed: usize,
    Draw: usize,
    Map: usize,
    Unmap: usize,
    PSSetConstantBuffers: usize,
    IASetInputLayout: usize,
    IASetVertexBuffers: usize,
    IASetIndexBuffer: usize,
    DrawIndexedInstanced: usize,
    DrawInstanced: usize,
    GSSetConstantBuffers: usize,
    GSSetShader: usize,
    IASetPrimitiveTopology: usize,
    VSSetShaderResources: usize,
    VSSetSamplers: usize,
    Begin: usize,
    End: usize,
    GetData: usize,
    SetPredication: usize,
    GSSetShaderResources: usize,
    GSSetSamplers: usize,
    OMSetRenderTargets: usize,
    OMSetRenderTargetsAndUnorderedAccessViews: usize,
    OMSetBlendState: usize,
    OMSetDepthStencilState: usize,
    SOSetTargets: usize,
    DrawAuto: usize,
    DrawIndexedInstancedIndirect: usize,
    DrawInstancedIndirect: usize,
    Dispatch: usize,
    DispatchIndirect: usize,
    RSSetState: usize,
    RSSetViewports: usize,
    RSSetScissorRects: usize,
    CopySubresourceRegion: usize,
    CopyResource: usize,
    UpdateSubresource: usize,
    CopyStructureCount: usize,
    ClearRenderTargetView: usize,
    ClearUnorderedAccessViewUint: usize,
    ClearUnorderedAccessViewFloat: usize,
    ClearDepthStencilView: usize,
    GenerateMips: usize,
    SetResourceMinLOD: usize,
    GetResourceMinLOD: usize,
    ResolveSubresource: usize,
    ExecuteCommandList: usize,
    HSSetShaderResources: usize,
    HSSetShader: usize,
    HSSetSamplers: usize,
    HSSetConstantBuffers: usize,
    DSSetShaderResources: usize,
    DSSetShader: usize,
    DSSetSamplers: usize,
    DSSetConstantBuffers: usize,
    CSSetShaderResources: usize,
    CSSetUnorderedAccessViews: usize,
    CSSetShader: usize,
    CSSetSamplers: usize,
    CSSetConstantBuffers: usize,
    VSGetConstantBuffers: usize,
    PSGetShaderResources: usize,
    PSGetShader: usize,
    PSGetSamplers: usize,
    VSGetShader: usize,
    PSGetConstantBuffers: usize,
    IAGetInputLayout: usize,
    IAGetVertexBuffers: usize,
    IAGetIndexBuffer: usize,
    GSGetConstantBuffers: usize,
    GSGetShader: usize,
    IAGetPrimitiveTopology: usize,
    VSGetShaderResources: usize,
    VSGetSamplers: usize,
    GetPredication: usize,
    GSGetShaderResources: usize,
    GSGetSamplers: usize,
    OMGetRenderTargets: usize,
    OMGetRenderTargetsAndUnorderedAccessViews: usize,
    OMGetBlendState: usize,
    OMGetDepthStencilState: usize,
    SOGetTargets: usize,
    RSGetState: usize,
    RSGetViewports: usize,
    RSGetScissorRects: usize,
    HSGetShaderResources: usize,
    HSGetShader: usize,
    HSGetSamplers: usize,
    HSGetConstantBuffers: usize,
    DSGetShaderResources: usize,
    DSGetShader: usize,
    DSGetSamplers: usize,
    DSGetConstantBuffers: usize,
    CSGetShaderResources: usize,
    CSGetUnorderedAccessViews: usize,
    CSGetShader: usize,
    CSGetSamplers: usize,
    CSGetConstantBuffers: usize,
    ClearState: usize,
    Flush: usize,
    GetType: usize,
    GetContextFlags: usize,
    FinishCommandList: usize,
}
windows_core::imp::define_interface!(
    IDXGIAdapter,
    IDXGIAdapter_Vtbl,
    0x2411e7e1_12ac_4ccf_bd14_9798e8534dc0
);
impl core::ops::Deref for IDXGIAdapter {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIAdapter, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIAdapter_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    EnumOutputs: usize,
    GetDesc: usize,
    CheckInterfaceSupport: usize,
}
windows_core::imp::define_interface!(
    IDXGIDevice,
    IDXGIDevice_Vtbl,
    0x54ec77fa_1377_44e6_8c32_88fd5f44c84c
);
impl core::ops::Deref for IDXGIDevice {
    type Target = IDXGIObject;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
windows_core::imp::interface_hierarchy!(IDXGIDevice, windows_core::IUnknown, IDXGIObject);
#[repr(C)]
pub struct IDXGIDevice_Vtbl {
    pub base__: IDXGIObject_Vtbl,
    GetAdapter: usize,
    CreateSurface: usize,
    QueryResourceResidency: usize,
    SetGPUThreadPriority: usize,
    GetGPUThreadPriority: usize,
}
windows_core::imp::define_interface!(
    IDXGIObject,
    IDXGIObject_Vtbl,
    0xaec22fb8_76f3_4639_9be0_28eb43a67a2e
);
windows_core::imp::interface_hierarchy!(IDXGIObject, windows_core::IUnknown);
#[repr(C)]
pub struct IDXGIObject_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    SetPrivateData: usize,
    SetPrivateDataInterface: usize,
    GetPrivateData: usize,
    GetParent: usize,
}
pub const IDYES: i32 = 6;
windows_core::imp::define_interface!(
    IDataPackage,
    IDataPackage_Vtbl,
    0x61ebf5c7_efea_4346_9554_981d7e198ffe
);
impl windows_core::RuntimeType for IDataPackage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDataPackage {
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IDataPackage_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    GetView: usize,
    Properties: usize,
    RequestedOperation: usize,
    SetRequestedOperation: usize,
    OperationCompleted: usize,
    RemoveOperationCompleted: usize,
    Destroyed: usize,
    RemoveDestroyed: usize,
    SetData: usize,
    SetDataProvider: usize,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataPackageView,
    IDataPackageView_Vtbl,
    0x7b840471_5900_4d85_a90b_10cb85fe3552
);
impl windows_core::RuntimeType for IDataPackageView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDataPackageView {
    pub fn GetTextAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetTextAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDataPackageView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Properties: usize,
    RequestedOperation: usize,
    ReportOperationCompleted: usize,
    AvailableFormats: usize,
    Contains: usize,
    GetDataAsync: usize,
    pub GetTextAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataReader,
    IDataReader_Vtbl,
    0xe2b50029_b4c1_4314_a4b8_fb813a2f275e
);
impl windows_core::RuntimeType for IDataReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IDataReader,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IDataReader {
    pub fn UnconsumedBufferLength(&self) -> windows_core::Result<u32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).UnconsumedBufferLength)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReadBytes(&self, value: &mut [u8]) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).ReadBytes)(
                windows_core::Interface::as_raw(self),
                value.len().try_into().unwrap(),
                value.as_mut_ptr(),
            )
            .ok()
        }
    }
    pub fn LoadAsync(&self, count: u32) -> windows_core::Result<DataReaderLoadOperation> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).LoadAsync)(
                windows_core::Interface::as_raw(self),
                count,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDataReader_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub UnconsumedBufferLength:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut u32) -> windows_core::HRESULT,
    UnicodeEncoding: usize,
    SetUnicodeEncoding: usize,
    ByteOrder: usize,
    SetByteOrder: usize,
    InputStreamOptions: usize,
    SetInputStreamOptions: usize,
    ReadByte: usize,
    pub ReadBytes:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, *mut u8) -> windows_core::HRESULT,
    ReadBuffer: usize,
    ReadBoolean: usize,
    ReadGuid: usize,
    ReadInt16: usize,
    ReadInt32: usize,
    ReadInt64: usize,
    ReadUInt16: usize,
    ReadUInt32: usize,
    ReadUInt64: usize,
    ReadSingle: usize,
    ReadDouble: usize,
    ReadString: usize,
    ReadDateTime: usize,
    ReadTimeSpan: usize,
    pub LoadAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataReaderFactory,
    IDataReaderFactory_Vtbl,
    0xd7527847_57da_4e15_914c_06806699a098
);
impl windows_core::RuntimeType for IDataReaderFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDataReaderFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateDataReader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataReaderStatics,
    IDataReaderStatics_Vtbl,
    0x11fcbfc8_f93a_471b_b121_f379e349313c
);
impl windows_core::RuntimeType for IDataReaderStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDataReaderStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub FromBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataWriter,
    IDataWriter_Vtbl,
    0x64b89265_d341_4922_b38a_dd4af8808c4e
);
impl windows_core::RuntimeType for IDataWriter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IDataWriter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IDataWriter {
    pub fn WriteBytes(&self, value: &[u8]) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).WriteBytes)(
                windows_core::Interface::as_raw(self),
                value.len().try_into().unwrap(),
                value.as_ptr(),
            )
            .ok()
        }
    }
    pub fn StoreAsync(&self) -> windows_core::Result<DataWriterStoreOperation> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).StoreAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDataWriter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    UnstoredBufferLength: usize,
    UnicodeEncoding: usize,
    SetUnicodeEncoding: usize,
    ByteOrder: usize,
    SetByteOrder: usize,
    WriteByte: usize,
    pub WriteBytes:
        unsafe extern "system" fn(*mut core::ffi::c_void, u32, *const u8) -> windows_core::HRESULT,
    WriteBuffer: usize,
    WriteBufferRange: usize,
    WriteBoolean: usize,
    WriteGuid: usize,
    WriteInt16: usize,
    WriteInt32: usize,
    WriteInt64: usize,
    WriteUInt16: usize,
    WriteUInt32: usize,
    WriteUInt64: usize,
    WriteSingle: usize,
    WriteDouble: usize,
    WriteDateTime: usize,
    WriteTimeSpan: usize,
    WriteString: usize,
    MeasureString: usize,
    pub StoreAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDataWriterFactory,
    IDataWriterFactory_Vtbl,
    0x338c67c2_8b84_4c2b_9c50_7b8767847a1f
);
impl windows_core::RuntimeType for IDataWriterFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDataWriterFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateDataWriter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDateTimeFormatter,
    IDateTimeFormatter_Vtbl,
    0x95eeca10_73e0_4e4b_a183_3d6ad0ba35ec
);
impl windows_core::RuntimeType for IDateTimeFormatter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDateTimeFormatter {
    pub fn Format(&self, value: DateTime) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Format)(
                windows_core::Interface::as_raw(self),
                value,
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct IDateTimeFormatter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Languages: usize,
    GeographicRegion: usize,
    Calendar: usize,
    Clock: usize,
    NumeralSystem: usize,
    SetNumeralSystem: usize,
    Patterns: usize,
    Template: usize,
    pub Format: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        DateTime,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDateTimeFormatterFactory,
    IDateTimeFormatterFactory_Vtbl,
    0xec8d8a53_1a2e_412d_8815_3b745fb1a2a0
);
impl windows_core::RuntimeType for IDateTimeFormatterFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDateTimeFormatterFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateDateTimeFormatter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDeferral,
    IDeferral_Vtbl,
    0xd6269732_3b7f_46a7_b40b_4fdca2a2c693
);
impl windows_core::RuntimeType for IDeferral {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDeferral {
    pub fn Complete(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Complete)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IDeferral_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Complete: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDependencyObject,
    IDependencyObject_Vtbl,
    0xe7beaee7_160e_50f7_8789_d63463f979fa
);
impl windows_core::RuntimeType for IDependencyObject {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDependencyObject_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDirect3D11CaptureFrame,
    IDirect3D11CaptureFrame_Vtbl,
    0xfa50c623_38da_4b32_acf3_fa9734ad800e
);
impl windows_core::RuntimeType for IDirect3D11CaptureFrame {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDirect3D11CaptureFrame {
    pub fn Surface(&self) -> windows_core::Result<IDirect3DSurface> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Surface)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDirect3D11CaptureFrame_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Surface: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDirect3D11CaptureFramePool,
    IDirect3D11CaptureFramePool_Vtbl,
    0x24eb6d22_1975_422e_82e7_780dbd8ddf24
);
impl windows_core::RuntimeType for IDirect3D11CaptureFramePool {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDirect3D11CaptureFramePool {
    pub fn TryGetNextFrame(&self) -> windows_core::Result<Direct3D11CaptureFrame> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TryGetNextFrame)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn CreateCaptureSession<P0>(&self, item: P0) -> windows_core::Result<GraphicsCaptureSession>
    where
        P0: windows_core::Param<GraphicsCaptureItem>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CreateCaptureSession)(
                windows_core::Interface::as_raw(self),
                item.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IDirect3D11CaptureFramePool_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Recreate: usize,
    pub TryGetNextFrame: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    FrameArrived: usize,
    RemoveFrameArrived: usize,
    pub CreateCaptureSession: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDirect3D11CaptureFramePoolStatics2,
    IDirect3D11CaptureFramePoolStatics2_Vtbl,
    0x589b103f_6bbc_5df5_a991_02e28b3b66d5
);
impl windows_core::RuntimeType for IDirect3D11CaptureFramePoolStatics2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDirect3D11CaptureFramePoolStatics2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateFreeThreaded: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        DirectXPixelFormat,
        i32,
        SizeInt32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDirect3DDevice,
    IDirect3DDevice_Vtbl,
    0xa37624ab_8d5f_4650_9d3e_9eae3d9bc670
);
impl windows_core::RuntimeType for IDirect3DDevice {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IDirect3DDevice,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(IDirect3DDevice, IClosable);
impl IDirect3DDevice {
    pub fn Trim(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Trim)(windows_core::Interface::as_raw(self)).ok()
        }
    }
}
#[repr(C)]
pub struct IDirect3DDevice_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Trim: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDirect3DSurface,
    IDirect3DSurface_Vtbl,
    0x0bf4a146_13c1_4694_bee3_7abf15eaf586
);
impl windows_core::RuntimeType for IDirect3DSurface {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IDirect3DSurface,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IDirect3DSurface_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IDispatcherQueue,
    IDispatcherQueue_Vtbl,
    0xf6ebf8fa_be1c_5bf6_a467_73da28738ae8
);
impl windows_core::RuntimeType for IDispatcherQueue {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDispatcherQueue {
    pub fn TryEnqueue<P0>(&self, callback: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<DispatcherQueueHandler>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TryEnqueue)(
                windows_core::Interface::as_raw(self),
                callback.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IDispatcherQueue_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    CreateTimer: usize,
    pub TryEnqueue: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDispatcherQueueStatics,
    IDispatcherQueueStatics_Vtbl,
    0xcd3382ea_a455_5124_b63a_ca40d34ca23c
);
impl windows_core::RuntimeType for IDispatcherQueueStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDispatcherQueueStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetForCurrentThread: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDisplayArea,
    IDisplayArea_Vtbl,
    0x5c7e0537_b621_5579_bcae_a84aa8746167
);
impl windows_core::RuntimeType for IDisplayArea {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDisplayArea {
    pub fn WorkArea(&self) -> windows_core::Result<RectInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).WorkArea)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IDisplayArea_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    DisplayId: usize,
    IsPrimary: usize,
    OuterBounds: usize,
    pub WorkArea:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut RectInt32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDisplayAreaStatics,
    IDisplayAreaStatics_Vtbl,
    0x02ab4926_211e_5d49_8e4b_2af193daed09
);
impl windows_core::RuntimeType for IDisplayAreaStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDisplayAreaStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Primary: usize,
    CreateWatcher: usize,
    FindAll: usize,
    GetFromWindowId: usize,
    GetFromPoint: usize,
    pub GetFromRect: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        RectInt32,
        DisplayAreaFallback,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDoubleAnimation,
    IDoubleAnimation_Vtbl,
    0x651ec97e_e483_5985_aa0b_49cfb07432dd
);
impl windows_core::RuntimeType for IDoubleAnimation {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IDoubleAnimation {
    pub fn SetFrom(&self, value: Option<f64>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<f64> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetFrom)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
    pub fn SetTo(&self, value: Option<f64>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<f64> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetTo)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IDoubleAnimation_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    From: usize,
    pub SetFrom: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    To: usize,
    pub SetTo: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IDragItemsCompletedEventArgs,
    IDragItemsCompletedEventArgs_Vtbl,
    0xc0138552_f467_5c3e_8af4_593607762844
);
impl windows_core::RuntimeType for IDragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IDragItemsCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFileOpenPicker,
    IFileOpenPicker_Vtbl,
    0x9d00f175_c783_51bd_8c93_fb63695d3abc
);
impl windows_core::RuntimeType for IFileOpenPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFileOpenPicker {
    pub fn FileTypeFilter(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::HSTRING>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).FileTypeFilter)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn PickSingleFileAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<PickFileResult>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PickSingleFileAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IFileOpenPicker_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ViewMode: usize,
    SetViewMode: usize,
    SuggestedStartLocation: usize,
    SetSuggestedStartLocation: usize,
    CommitButtonText: usize,
    SetCommitButtonText: usize,
    pub FileTypeFilter: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PickSingleFileAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFileOpenPickerFactory,
    IFileOpenPickerFactory_Vtbl,
    0x315e86d7_d7a2_5d81_b379_7af78207b1af
);
impl windows_core::RuntimeType for IFileOpenPickerFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFileOpenPickerFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowId,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFileSavePicker,
    IFileSavePicker_Vtbl,
    0x79f1f4df_741b_59b2_aa06_fe9ac817b7dd
);
impl windows_core::RuntimeType for IFileSavePicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFileSavePicker {
    pub fn SetSuggestedFileName(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSuggestedFileName)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetSuggestedFolder(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSuggestedFolder)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn PickSaveFileAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<PickFileResult>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PickSaveFileAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IFileSavePicker_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    SuggestedStartLocation: usize,
    SetSuggestedStartLocation: usize,
    CommitButtonText: usize,
    SetCommitButtonText: usize,
    FileTypeChoices: usize,
    DefaultFileExtension: usize,
    SetDefaultFileExtension: usize,
    SuggestedFileName: usize,
    pub SetSuggestedFileName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SuggestedFolder: usize,
    pub SetSuggestedFolder: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub PickSaveFileAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFileSavePickerFactory,
    IFileSavePickerFactory_Vtbl,
    0x2e256696_30b6_5a05_a8f5_c752db6dd268
);
impl windows_core::RuntimeType for IFileSavePickerFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFileSavePickerFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowId,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyout,
    IFlyout_Vtbl,
    0xd4a1eb7d_59b8_5df9_87c3_bd5e3856923f
);
impl windows_core::RuntimeType for IFlyout {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFlyout {
    pub fn Content(&self) -> windows_core::Result<UIElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IFlyout_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutBase,
    IFlyoutBase_Vtbl,
    0xbb6603bf_744d_5c31_a87d_744394634d77
);
impl windows_core::RuntimeType for IFlyoutBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFlyoutBase {
    pub fn SetPlacement(&self, value: FlyoutPlacementMode) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPlacement)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Target(&self) -> windows_core::Result<FrameworkElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Target)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetShouldConstrainToRootBounds(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetShouldConstrainToRootBounds)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetOverlayInputPassThroughElement<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetOverlayInputPassThroughElement)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn IsOpen(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsOpen)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Opened<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: EventHandler<windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<EventHandler<windows_core::IInspectable>, F>::new(
                    &EventHandlerBox::<windows_core::IInspectable, F>::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Opened)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveOpened,
            ))
        }
    }
    pub fn Closed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: EventHandler<windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<EventHandler<windows_core::IInspectable>, F>::new(
                    &EventHandlerBox::<windows_core::IInspectable, F>::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Closed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClosed,
            ))
        }
    }
    pub fn Opening<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: EventHandler<windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<EventHandler<windows_core::IInspectable>, F>::new(
                    &EventHandlerBox::<windows_core::IInspectable, F>::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Opening)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveOpening,
            ))
        }
    }
    pub fn ShowAt<P0>(&self, placementtarget: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FrameworkElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ShowAt)(
                windows_core::Interface::as_raw(self),
                placementtarget.param().abi(),
            )
            .ok()
        }
    }
    pub fn ShowAtWithOptions<P0, P1>(
        &self,
        placementtarget: P0,
        showoptions: P1,
    ) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
        P1: windows_core::Param<FlyoutShowOptions>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ShowAtWithOptions)(
                windows_core::Interface::as_raw(self),
                placementtarget.param().abi(),
                showoptions.param().abi(),
            )
            .ok()
        }
    }
    pub fn Hide(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Hide)(windows_core::Interface::as_raw(self)).ok()
        }
    }
}
#[repr(C)]
pub struct IFlyoutBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Placement: usize,
    pub SetPlacement: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        FlyoutPlacementMode,
    ) -> windows_core::HRESULT,
    pub Target: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    AllowFocusOnInteraction: usize,
    SetAllowFocusOnInteraction: usize,
    LightDismissOverlayMode: usize,
    SetLightDismissOverlayMode: usize,
    AllowFocusWhenDisabled: usize,
    SetAllowFocusWhenDisabled: usize,
    ShowMode: usize,
    SetShowMode: usize,
    InputDevicePrefersPrimaryCommands: usize,
    AreOpenCloseAnimationsEnabled: usize,
    SetAreOpenCloseAnimationsEnabled: usize,
    ShouldConstrainToRootBounds: usize,
    pub SetShouldConstrainToRootBounds:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    IsConstrainedToRootBounds: usize,
    ElementSoundMode: usize,
    SetElementSoundMode: usize,
    OverlayInputPassThroughElement: usize,
    pub SetOverlayInputPassThroughElement: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub IsOpen:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    XamlRoot: usize,
    SetXamlRoot: usize,
    pub Opened: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveOpened:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub Closed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClosed:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub Opening: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveOpening:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Closing: usize,
    RemoveClosing: usize,
    pub ShowAt: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub ShowAtWithOptions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Hide: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutShowOptions,
    IFlyoutShowOptions_Vtbl,
    0x30774a93_2803_50d3_b406_904aec3e175d
);
impl windows_core::RuntimeType for IFlyoutShowOptions {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFlyoutShowOptions {
    pub fn SetShowMode(&self, value: FlyoutShowMode) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetShowMode)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetPlacement(&self, value: FlyoutPlacementMode) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetPlacement)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IFlyoutShowOptions_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Position: usize,
    SetPosition: usize,
    ExclusionRect: usize,
    SetExclusionRect: usize,
    ShowMode: usize,
    pub SetShowMode:
        unsafe extern "system" fn(*mut core::ffi::c_void, FlyoutShowMode) -> windows_core::HRESULT,
    Placement: usize,
    pub SetPlacement: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        FlyoutPlacementMode,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFlyoutShowOptionsFactory,
    IFlyoutShowOptionsFactory_Vtbl,
    0x17426d30_70d9_54d7_bd39_e7c4c940c0f4
);
impl windows_core::RuntimeType for IFlyoutShowOptionsFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFlyoutShowOptionsFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFocusManager,
    IFocusManager_Vtbl,
    0x9fd07bc5_d2d4_53fe_a31a_846de8b7a257
);
impl windows_core::RuntimeType for IFocusManager {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFocusManager_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFocusManagerStatics,
    IFocusManagerStatics_Vtbl,
    0xe73dce04_e23a_5fb3_96ab_7df04c51dff2
);
impl windows_core::RuntimeType for IFocusManagerStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFocusManagerStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    GotFocus: usize,
    RemoveGotFocus: usize,
    LostFocus: usize,
    RemoveLostFocus: usize,
    GettingFocus: usize,
    RemoveGettingFocus: usize,
    LosingFocus: usize,
    RemoveLosingFocus: usize,
    TryFocusAsync: usize,
    TryMoveFocusAsync: usize,
    TryMoveFocusWithOptionsAsync: usize,
    TryMoveFocusWithOptions: usize,
    FindNextElement: usize,
    FindFirstFocusableElement: usize,
    FindLastFocusableElement: usize,
    FindNextElementWithOptions: usize,
    FindNextFocusableElement: usize,
    FindNextFocusableElementWithHint: usize,
    TryMoveFocus: usize,
    GetFocusedElement: usize,
    pub GetFocusedElementWithRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFolderPicker,
    IFolderPicker_Vtbl,
    0x3ef0d1ca_97c6_5873_8ea2_02c450174290
);
impl windows_core::RuntimeType for IFolderPicker {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFolderPicker {
    pub fn PickSingleFolderAsync(
        &self,
    ) -> windows_core::Result<windows_future::IAsyncOperation<PickFolderResult>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PickSingleFolderAsync)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IFolderPicker_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ViewMode: usize,
    SetViewMode: usize,
    SuggestedStartLocation: usize,
    SetSuggestedStartLocation: usize,
    CommitButtonText: usize,
    SetCommitButtonText: usize,
    pub PickSingleFolderAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFolderPickerFactory,
    IFolderPickerFactory_Vtbl,
    0xe1550d89_b389_5886_8395_022b1588d6a8
);
impl windows_core::RuntimeType for IFolderPickerFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFolderPickerFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowId,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFontIcon,
    IFontIcon_Vtbl,
    0x6eba5ed9_d233_5f5e_91a8_f5134292658a
);
impl windows_core::RuntimeType for IFontIcon {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFontIcon {
    pub fn Glyph(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Glyph)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetGlyph(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetGlyph)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IFontIcon_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Glyph: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetGlyph: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFontIconFactory,
    IFontIconFactory_Vtbl,
    0xaa9a24fe_bef8_564a_b200_694cd6f6ba4e
);
impl windows_core::RuntimeType for IFontIconFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFontIconFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFrameworkElement,
    IFrameworkElement_Vtbl,
    0xfe08f13d_dc6a_5495_ad44_c2d8d21863b0
);
impl windows_core::RuntimeType for IFrameworkElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IFrameworkElement {
    pub fn ActualWidth(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ActualWidth)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ActualHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ActualHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetHeight(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHeight)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMinWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMinWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMaxWidth(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMaxWidth)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetMinHeight(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMinHeight)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Name(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Name)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetRequestedTheme(&self, value: ElementTheme) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetRequestedTheme)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Loaded<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Loaded)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveLoaded,
            ))
        }
    }
    pub fn SizeChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<SizeChangedEventArgs>,
            ) + 'static,
    {
        let handler: SizeChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<SizeChangedEventHandler, F>::new(
                &SizeChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SizeChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSizeChanged,
            ))
        }
    }
    pub fn LayoutUpdated<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<windows_core::IInspectable>,
            ) + 'static,
    {
        let handler: EventHandler<windows_core::IInspectable> = {
            let com =
                windows_core::imp::DelegateBox::<EventHandler<windows_core::IInspectable>, F>::new(
                    &EventHandlerBox::<windows_core::IInspectable, F>::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).LayoutUpdated)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveLayoutUpdated,
            ))
        }
    }
    pub fn FindName(&self, name: &str) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).FindName)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(name)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IFrameworkElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Triggers: usize,
    Resources: usize,
    SetResources: usize,
    Tag: usize,
    SetTag: usize,
    Language: usize,
    SetLanguage: usize,
    pub ActualWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub ActualHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    Width: usize,
    pub SetWidth: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    Height: usize,
    pub SetHeight: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MinWidth: usize,
    pub SetMinWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MaxWidth: usize,
    pub SetMaxWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MinHeight: usize,
    pub SetMinHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    MaxHeight: usize,
    SetMaxHeight: usize,
    HorizontalAlignment: usize,
    SetHorizontalAlignment: usize,
    VerticalAlignment: usize,
    SetVerticalAlignment: usize,
    Margin: usize,
    SetMargin: usize,
    pub Name: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SetName: usize,
    BaseUri: usize,
    DataContext: usize,
    SetDataContext: usize,
    AllowFocusOnInteraction: usize,
    SetAllowFocusOnInteraction: usize,
    FocusVisualMargin: usize,
    SetFocusVisualMargin: usize,
    FocusVisualSecondaryThickness: usize,
    SetFocusVisualSecondaryThickness: usize,
    FocusVisualPrimaryThickness: usize,
    SetFocusVisualPrimaryThickness: usize,
    FocusVisualSecondaryBrush: usize,
    SetFocusVisualSecondaryBrush: usize,
    FocusVisualPrimaryBrush: usize,
    SetFocusVisualPrimaryBrush: usize,
    AllowFocusWhenDisabled: usize,
    SetAllowFocusWhenDisabled: usize,
    Style: usize,
    SetStyle: usize,
    Parent: usize,
    FlowDirection: usize,
    SetFlowDirection: usize,
    RequestedTheme: usize,
    pub SetRequestedTheme:
        unsafe extern "system" fn(*mut core::ffi::c_void, ElementTheme) -> windows_core::HRESULT,
    IsLoaded: usize,
    ActualTheme: usize,
    pub Loaded: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveLoaded:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Unloaded: usize,
    RemoveUnloaded: usize,
    DataContextChanged: usize,
    RemoveDataContextChanged: usize,
    pub SizeChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSizeChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub LayoutUpdated: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveLayoutUpdated:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    Loading: usize,
    RemoveLoading: usize,
    ActualThemeChanged: usize,
    RemoveActualThemeChanged: usize,
    EffectiveViewportChanged: usize,
    RemoveEffectiveViewportChanged: usize,
    pub FindName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IFrameworkElementAutomationPeer,
    IFrameworkElementAutomationPeer_Vtbl,
    0x7dab4f24_605c_51cb_87db_3eed1b9fb37b
);
impl windows_core::RuntimeType for IFrameworkElementAutomationPeer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFrameworkElementAutomationPeer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IFrameworkElementAutomationPeerStatics,
    IFrameworkElementAutomationPeerStatics_Vtbl,
    0x081f6fbe_6500_528a_a506_f5a4d41ddf6c
);
impl windows_core::RuntimeType for IFrameworkElementAutomationPeerStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IFrameworkElementAutomationPeerStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FromElement: usize,
    pub CreatePeerForElement: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGeneralTransform,
    IGeneralTransform_Vtbl,
    0x04eedeeb_31e5_54c0_ae3f_8bd06645d339
);
impl windows_core::RuntimeType for IGeneralTransform {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGeneralTransform {
    pub fn TransformPoint(&self, point: Point) -> windows_core::Result<Point> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TransformPoint)(
                windows_core::Interface::as_raw(self),
                point,
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IGeneralTransform_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Inverse: usize,
    pub TransformPoint: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        Point,
        *mut Point,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGraphicsCaptureItem,
    IGraphicsCaptureItem_Vtbl,
    0x79c3f95b_31f7_4ec2_a464_632ef5d30760
);
impl windows_core::RuntimeType for IGraphicsCaptureItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGraphicsCaptureItem {
    pub fn Size(&self) -> windows_core::Result<SizeInt32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Size)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IGraphicsCaptureItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    DisplayName: usize,
    pub Size:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut SizeInt32) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGraphicsCaptureItemInterop,
    IGraphicsCaptureItemInterop_Vtbl,
    0x3628e81b_3cac_4c60_b7f4_23ce0e0c3356
);
windows_core::imp::interface_hierarchy!(IGraphicsCaptureItemInterop, windows_core::IUnknown);
impl IGraphicsCaptureItemInterop {
    pub unsafe fn CreateForWindow<T>(&self, window: HWND) -> windows_core::Result<T>
    where
        T: windows_core::Interface,
    {
        let mut result__ = core::ptr::null_mut();
        unsafe {
            (windows_core::Interface::vtable(self).CreateForWindow)(
                windows_core::Interface::as_raw(self),
                window,
                &T::IID,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IGraphicsCaptureItemInterop_Vtbl {
    pub base__: windows_core::IUnknown_Vtbl,
    pub CreateForWindow: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        HWND,
        *const windows_core::GUID,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CreateForMonitor: usize,
}
windows_core::imp::define_interface!(
    IGraphicsCaptureSession,
    IGraphicsCaptureSession_Vtbl,
    0x814e42a9_f70f_4ad7_939b_fddcc6eb880d
);
impl windows_core::RuntimeType for IGraphicsCaptureSession {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGraphicsCaptureSession {
    pub fn StartCapture(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).StartCapture)(windows_core::Interface::as_raw(
                self,
            ))
            .ok()
        }
    }
}
#[repr(C)]
pub struct IGraphicsCaptureSession_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub StartCapture: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGraphicsCaptureSession2,
    IGraphicsCaptureSession2_Vtbl,
    0x2c39ae40_7d2e_5044_804e_8b6799d4cf9e
);
impl windows_core::RuntimeType for IGraphicsCaptureSession2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGraphicsCaptureSession2 {
    pub fn SetIsCursorCaptureEnabled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsCursorCaptureEnabled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IGraphicsCaptureSession2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsCursorCaptureEnabled: usize,
    pub SetIsCursorCaptureEnabled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGraphicsCaptureSession3,
    IGraphicsCaptureSession3_Vtbl,
    0xf2cdd966_22ae_5ea1_9596_3a289344c3be
);
impl windows_core::RuntimeType for IGraphicsCaptureSession3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGraphicsCaptureSession3 {
    pub fn SetIsBorderRequired(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsBorderRequired)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IGraphicsCaptureSession3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsBorderRequired: usize,
    pub SetIsBorderRequired:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(IGrid, IGrid_Vtbl, 0xc4496219_9014_58a1_b4ad_c5044913a5bb);
impl windows_core::RuntimeType for IGrid {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IGrid {
    pub fn ColumnDefinitions(&self) -> windows_core::Result<ColumnDefinitionCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ColumnDefinitions)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IGrid_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    RowDefinitions: usize,
    pub ColumnDefinitions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IGridStatics,
    IGridStatics_Vtbl,
    0xef9cf81d_a431_50f4_abf5_3023fe447704
);
impl windows_core::RuntimeType for IGridStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IGridStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    BackgroundSizingProperty: usize,
    BorderBrushProperty: usize,
    BorderThicknessProperty: usize,
    CornerRadiusProperty: usize,
    PaddingProperty: usize,
    RowSpacingProperty: usize,
    ColumnSpacingProperty: usize,
    RowProperty: usize,
    GetRow: usize,
    SetRow: usize,
    ColumnProperty: usize,
    GetColumn: usize,
    pub SetColumn: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        i32,
    ) -> windows_core::HRESULT,
    RowSpanProperty: usize,
    GetRowSpan: usize,
    SetRowSpan: usize,
    ColumnSpanProperty: usize,
    GetColumnSpan: usize,
    pub SetColumnSpan: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        i32,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IIconElement,
    IIconElement_Vtbl,
    0x18f69350_279e_50ea_8d23_138e717ed939
);
impl windows_core::RuntimeType for IIconElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IIconElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(IImage, IImage_Vtbl, 0x220d3d8d_66de_53a1_a215_ba9c165565ab);
impl windows_core::RuntimeType for IImage {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IImage {
    pub fn SetSource<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ImageSource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSource)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IImage_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Source: usize,
    pub SetSource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IImageIcon,
    IImageIcon_Vtbl,
    0x78a7b526_e635_59c6_93a1_d7e3c2fac6d5
);
impl windows_core::RuntimeType for IImageIcon {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IImageIcon {
    pub fn SetSource<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<ImageSource>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSource)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IImageIcon_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Source: usize,
    pub SetSource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IImageIconFactory,
    IImageIconFactory_Vtbl,
    0x235e0279_a7d0_5fda_a308_9b7cb9c4c912
);
impl windows_core::RuntimeType for IImageIconFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IImageIconFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IImageSource,
    IImageSource_Vtbl,
    0x6c2038f6_d6d5_55e9_9b9e_082f12dbff60
);
impl windows_core::RuntimeType for IImageSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IImageSource_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInfoBar,
    IInfoBar_Vtbl,
    0xc1c3a438_dd79_5d22_9e42_5a3cdf8113a9
);
impl windows_core::RuntimeType for IInfoBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IInfoBar {
    pub fn IsOpen(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsOpen)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsOpen(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsOpen)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Title(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Title)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetTitle(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTitle)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetMessage(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMessage)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetSeverity(&self, value: InfoBarSeverity) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSeverity)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IInfoBar_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsOpen:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsOpen: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub Title: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetTitle: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Message: usize,
    pub SetMessage: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Severity: usize,
    pub SetSeverity:
        unsafe extern "system" fn(*mut core::ffi::c_void, InfoBarSeverity) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInputCursor,
    IInputCursor_Vtbl,
    0x359b15f9_19c2_5714_8432_75176826406b
);
impl windows_core::RuntimeType for IInputCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IInputCursor_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputNonClientPointerSource,
    IInputNonClientPointerSource_Vtbl,
    0x471732b4_3d07_5104_b192_ebacf71e86df
);
impl windows_core::RuntimeType for IInputNonClientPointerSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IInputNonClientPointerSource {
    pub fn SetRegionRects(
        &self,
        region: NonClientRegionKind,
        rects: &[RectInt32],
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetRegionRects)(
                windows_core::Interface::as_raw(self),
                region,
                rects.len().try_into().unwrap(),
                rects.as_ptr(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IInputNonClientPointerSource_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    DispatcherQueue: usize,
    ClearAllRegionRects: usize,
    ClearRegionRects: usize,
    GetRegionRects: usize,
    pub SetRegionRects: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        NonClientRegionKind,
        u32,
        *const RectInt32,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInputNonClientPointerSourceStatics,
    IInputNonClientPointerSourceStatics_Vtbl,
    0x7d0b775c_1903_5dc7_bd2f_7a4b31f0cff2
);
impl windows_core::RuntimeType for IInputNonClientPointerSourceStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IInputNonClientPointerSourceStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetForWindowId: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        WindowId,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInputStream,
    IInputStream_Vtbl,
    0x905a0fe2_bc53_11df_8c49_001e4fc686da
);
impl windows_core::RuntimeType for IInputStream {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IInputStream,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IInputStream_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputSystemCursor,
    IInputSystemCursor_Vtbl,
    0x59f538e7_c500_59ab_8b54_0bc6100fd49e
);
impl windows_core::RuntimeType for IInputSystemCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IInputSystemCursor_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IInputSystemCursorStatics,
    IInputSystemCursorStatics_Vtbl,
    0xd3860bb6_698a_5814_aedd_c2fa8bba5a02
);
impl windows_core::RuntimeType for IInputSystemCursorStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IInputSystemCursorStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Create: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        InputSystemCursorShape,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IInvokeProvider,
    IInvokeProvider_Vtbl,
    0x02481105_3378_544d_b4e1_a1b368afbc02
);
impl windows_core::RuntimeType for IInvokeProvider {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IInvokeProvider,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IInvokeProvider {
    pub fn Invoke(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Invoke)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IInvokeProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Invoke: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IItemClickEventArgs,
    IItemClickEventArgs_Vtbl,
    0x1cf87a70_6348_57ec_9eac_fa0565adc60f
);
impl windows_core::RuntimeType for IItemClickEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IItemClickEventArgs {
    pub fn ClickedItem(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ClickedItem)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IItemClickEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub ClickedItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IItemsControl,
    IItemsControl_Vtbl,
    0xbf1ccb54_83e2_5b98_acbc_736f876c3d35
);
impl windows_core::RuntimeType for IItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IItemsControl {
    pub fn SetItemsSource<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetItemsSource)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Items(&self) -> windows_core::Result<ItemCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IItemsControl_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ItemsSource: usize,
    pub SetItemsSource: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IKeyRoutedEventArgs,
    IKeyRoutedEventArgs_Vtbl,
    0xee357007_a2d6_5c75_9431_05fd66ec7915
);
impl windows_core::RuntimeType for IKeyRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IKeyRoutedEventArgs {
    pub fn Key(&self) -> windows_core::Result<VirtualKey> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Key)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IKeyRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Key:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut VirtualKey) -> windows_core::HRESULT,
    KeyStatus: usize,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IKeyboardAccelerator,
    IKeyboardAccelerator_Vtbl,
    0x6f8bf1e2_4e91_5cf9_a6be_4770caf3d770
);
impl windows_core::RuntimeType for IKeyboardAccelerator {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IKeyboardAccelerator {
    pub fn Key(&self) -> windows_core::Result<VirtualKey> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Key)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetKey(&self, value: VirtualKey) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetKey)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Modifiers(&self) -> windows_core::Result<VirtualKeyModifiers> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Modifiers)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetModifiers(&self, value: VirtualKeyModifiers) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetModifiers)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Invoked<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<KeyboardAccelerator>,
                windows_core::Ref<KeyboardAcceleratorInvokedEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<KeyboardAccelerator, KeyboardAcceleratorInvokedEventArgs> = {
            let com =
                windows_core::imp::DelegateBox::<
                    TypedEventHandler<KeyboardAccelerator, KeyboardAcceleratorInvokedEventArgs>,
                    F,
                >::new(
                    &TypedEventHandlerBox::<
                        KeyboardAccelerator,
                        KeyboardAcceleratorInvokedEventArgs,
                        F,
                    >::VTABLE,
                    handler,
                );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Invoked)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveInvoked,
            ))
        }
    }
}
#[repr(C)]
pub struct IKeyboardAccelerator_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Key:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut VirtualKey) -> windows_core::HRESULT,
    pub SetKey:
        unsafe extern "system" fn(*mut core::ffi::c_void, VirtualKey) -> windows_core::HRESULT,
    pub Modifiers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut VirtualKeyModifiers,
    ) -> windows_core::HRESULT,
    pub SetModifiers: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        VirtualKeyModifiers,
    ) -> windows_core::HRESULT,
    IsEnabled: usize,
    SetIsEnabled: usize,
    ScopeOwner: usize,
    SetScopeOwner: usize,
    pub Invoked: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveInvoked:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IKeyboardAcceleratorFactory,
    IKeyboardAcceleratorFactory_Vtbl,
    0xca1d410a_af2a_51b9_a1de_6c0af9f3b598
);
impl windows_core::RuntimeType for IKeyboardAcceleratorFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IKeyboardAcceleratorFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IKeyboardAcceleratorInvokedEventArgs,
    IKeyboardAcceleratorInvokedEventArgs_Vtbl,
    0x62c9fdb0_b574_527d_97eb_5c7f674441e0
);
impl windows_core::RuntimeType for IKeyboardAcceleratorInvokedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IKeyboardAcceleratorInvokedEventArgs {
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IKeyboardAcceleratorInvokedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ILaunchActivatedEventArgs,
    ILaunchActivatedEventArgs_Vtbl,
    0xd505cea9_1bcb_5b29_a8be_944e00f06f78
);
impl windows_core::RuntimeType for ILaunchActivatedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ILaunchActivatedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListView,
    IListView_Vtbl,
    0xf6015db1_df63_52fd_a164_0df44715ee0a
);
impl windows_core::RuntimeType for IListView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IListView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewBase,
    IListViewBase_Vtbl,
    0x775c57ac_abce_5beb_8e34_3b8158aedd80
);
impl windows_core::RuntimeType for IListViewBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IListViewBase {
    pub fn ItemClick<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<ItemClickEventArgs>)
            + 'static,
    {
        let handler: ItemClickEventHandler = {
            let com = windows_core::imp::DelegateBox::<ItemClickEventHandler, F>::new(
                &ItemClickEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ItemClick)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveItemClick,
            ))
        }
    }
    pub fn DragItemsCompleted<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<ListViewBase>, windows_core::Ref<DragItemsCompletedEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<ListViewBase, DragItemsCompletedEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<ListViewBase, DragItemsCompletedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<ListViewBase, DragItemsCompletedEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DragItemsCompleted)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDragItemsCompleted,
            ))
        }
    }
    pub fn ContainerContentChanging<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<ListViewBase>,
                windows_core::Ref<ContainerContentChangingEventArgs>,
            ) + 'static,
    {
        let handler: TypedEventHandler<ListViewBase, ContainerContentChangingEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<ListViewBase, ContainerContentChangingEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<ListViewBase, ContainerContentChangingEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).ContainerContentChanging)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveContainerContentChanging,
            ))
        }
    }
    pub fn ScrollIntoView<P0>(&self, item: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ScrollIntoView)(
                windows_core::Interface::as_raw(self),
                item.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IListViewBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    SelectedItems: usize,
    SelectionMode: usize,
    SetSelectionMode: usize,
    IsSwipeEnabled: usize,
    SetIsSwipeEnabled: usize,
    CanDragItems: usize,
    SetCanDragItems: usize,
    CanReorderItems: usize,
    SetCanReorderItems: usize,
    IsItemClickEnabled: usize,
    SetIsItemClickEnabled: usize,
    DataFetchSize: usize,
    SetDataFetchSize: usize,
    IncrementalLoadingThreshold: usize,
    SetIncrementalLoadingThreshold: usize,
    IncrementalLoadingTrigger: usize,
    SetIncrementalLoadingTrigger: usize,
    ShowsScrollingPlaceholders: usize,
    SetShowsScrollingPlaceholders: usize,
    ReorderMode: usize,
    SetReorderMode: usize,
    SelectedRanges: usize,
    IsMultiSelectCheckBoxEnabled: usize,
    SetIsMultiSelectCheckBoxEnabled: usize,
    SingleSelectionFollowsFocus: usize,
    SetSingleSelectionFollowsFocus: usize,
    pub ItemClick: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveItemClick:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    DragItemsStarting: usize,
    RemoveDragItemsStarting: usize,
    pub DragItemsCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub ContainerContentChanging: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveContainerContentChanging:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    ChoosingItemContainer: usize,
    RemoveChoosingItemContainer: usize,
    ChoosingGroupHeaderContainer: usize,
    RemoveChoosingGroupHeaderContainer: usize,
    pub ScrollIntoView: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IListViewItem,
    IListViewItem_Vtbl,
    0x05fe41c2_0451_5d38_9c55_5d10cfd08889
);
impl windows_core::RuntimeType for IListViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IListViewItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IListViewItemFactory,
    IListViewItemFactory_Vtbl,
    0xd9f4d0b8_ee59_5036_bd7a_7c89cf0bc2ac
);
impl windows_core::RuntimeType for IListViewItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IListViewItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
pub const IMAGE_ICON: i32 = 1;
windows_core::imp::define_interface!(
    IMenuFlyout,
    IMenuFlyout_Vtbl,
    0xf4c77c6c_1fa5_5d85_8559_5d02b7d4e5e7
);
impl windows_core::RuntimeType for IMenuFlyout {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IMenuFlyout {
    pub fn Items(&self) -> windows_core::Result<windows_collections::IVector<MenuFlyoutItemBase>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IMenuFlyout_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMenuFlyoutFactory,
    IMenuFlyoutFactory_Vtbl,
    0xa3d225de_6b35_5442_b6c9_06fd24139a63
);
impl windows_core::RuntimeType for IMenuFlyoutFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMenuFlyoutFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMenuFlyoutItem,
    IMenuFlyoutItem_Vtbl,
    0x4252df5a_44f9_5ee8_b1cc_53de9aaa4095
);
impl windows_core::RuntimeType for IMenuFlyoutItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IMenuFlyoutItem {
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn Icon(&self) -> windows_core::Result<IconElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Icon)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetIcon<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<IconElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetIcon)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn KeyboardAcceleratorTextOverride(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).KeyboardAcceleratorTextOverride)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetKeyboardAcceleratorTextOverride(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetKeyboardAcceleratorTextOverride)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn Click<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Click)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClick,
            ))
        }
    }
}
#[repr(C)]
pub struct IMenuFlyoutItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Command: usize,
    SetCommand: usize,
    CommandParameter: usize,
    SetCommandParameter: usize,
    pub Icon: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetIcon: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub KeyboardAcceleratorTextOverride: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetKeyboardAcceleratorTextOverride: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    TemplateSettings: usize,
    pub Click: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClick:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMenuFlyoutItemBase,
    IMenuFlyoutItemBase_Vtbl,
    0x4bee2715_44a1_5f94_86e8_02ddbe3dc6b9
);
impl windows_core::RuntimeType for IMenuFlyoutItemBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMenuFlyoutItemBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IMenuFlyoutItemFactory,
    IMenuFlyoutItemFactory_Vtbl,
    0x9c3c9a1f_89af_521a_81a5_8a01db7a79af
);
impl windows_core::RuntimeType for IMenuFlyoutItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMenuFlyoutItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMenuFlyoutSeparator,
    IMenuFlyoutSeparator_Vtbl,
    0x3eaf5fd5_935e_5ed7_8d05_f6bafa936d25
);
impl windows_core::RuntimeType for IMenuFlyoutSeparator {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMenuFlyoutSeparator_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IMenuFlyoutSeparatorFactory,
    IMenuFlyoutSeparatorFactory_Vtbl,
    0x26156c9c_95ef_5e55_8342_773fc43baac3
);
impl windows_core::RuntimeType for IMenuFlyoutSeparatorFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMenuFlyoutSeparatorFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMenuFlyoutSubItem,
    IMenuFlyoutSubItem_Vtbl,
    0x6b0688c1_47b0_53b5_b6f9_5ec5d6623b84
);
impl windows_core::RuntimeType for IMenuFlyoutSubItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IMenuFlyoutSubItem {
    pub fn Items(&self) -> windows_core::Result<windows_collections::IVector<MenuFlyoutItemBase>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn Icon(&self) -> windows_core::Result<IconElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Icon)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetIcon<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<IconElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetIcon)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IMenuFlyoutSubItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Icon: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetIcon: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IMicaBackdrop,
    IMicaBackdrop_Vtbl,
    0xc156a404_3dac_593a_b1f3_7a33c289dc83
);
impl windows_core::RuntimeType for IMicaBackdrop {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMicaBackdrop_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IMicaBackdropFactory,
    IMicaBackdropFactory_Vtbl,
    0x774379ce_74bd_59d4_849d_d99c4184d838
);
impl windows_core::RuntimeType for IMicaBackdropFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IMicaBackdropFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct INPUT {
    pub r#type: u32,
    pub Anonymous: INPUT_0,
}
impl Default for INPUT {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy)]
pub union INPUT_0 {
    pub mi: MOUSEINPUT,
    pub ki: KEYBDINPUT,
    pub hi: HARDWAREINPUT,
}
impl Default for INPUT_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
pub const INPUT_KEYBOARD: i32 = 1;
windows_core::imp::define_interface!(
    IOutputStream,
    IOutputStream_Vtbl,
    0x905a0fe6_bc53_11df_8c49_001e4fc686da
);
impl windows_core::RuntimeType for IOutputStream {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IOutputStream,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IOutputStream_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IOverlappedPresenter,
    IOverlappedPresenter_Vtbl,
    0x21693970_4f4c_5172_9e9d_682a2d174884
);
impl windows_core::RuntimeType for IOverlappedPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IOverlappedPresenter {
    pub fn State(&self) -> windows_core::Result<OverlappedPresenterState> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).State)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Maximize(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Maximize)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn Restore(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Restore)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IOverlappedPresenter_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    HasBorder: usize,
    HasTitleBar: usize,
    IsAlwaysOnTop: usize,
    SetIsAlwaysOnTop: usize,
    IsMaximizable: usize,
    SetIsMaximizable: usize,
    IsMinimizable: usize,
    SetIsMinimizable: usize,
    IsModal: usize,
    SetIsModal: usize,
    IsResizable: usize,
    SetIsResizable: usize,
    pub State: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut OverlappedPresenterState,
    ) -> windows_core::HRESULT,
    pub Maximize: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    Minimize: usize,
    pub Restore: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IOverlappedPresenter3,
    IOverlappedPresenter3_Vtbl,
    0x55d26138_4c38_57e7_a0c1_d467b774db8c
);
impl windows_core::RuntimeType for IOverlappedPresenter3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IOverlappedPresenter3 {
    pub fn SetPreferredMinimumHeight(&self, value: Option<i32>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<i32> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredMinimumHeight)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
    pub fn SetPreferredMinimumWidth(&self, value: Option<i32>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<i32> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetPreferredMinimumWidth)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IOverlappedPresenter3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PreferredMinimumHeight: usize,
    pub SetPreferredMinimumHeight: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    PreferredMinimumWidth: usize,
    pub SetPreferredMinimumWidth: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(IPanel, IPanel_Vtbl, 0x27a1b418_56f3_525e_b883_cefed905eed3);
impl windows_core::RuntimeType for IPanel {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPanel {
    pub fn Children(&self) -> windows_core::Result<UIElementCollection> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Children)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IPanel_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Children: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPickFileResult,
    IPickFileResult_Vtbl,
    0xe6f2e3d6_7bb0_5d81_9e7d_6fd35a1f25ab
);
impl windows_core::RuntimeType for IPickFileResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPickFileResult {
    pub fn Path(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Path)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct IPickFileResult_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Path: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPickFolderResult,
    IPickFolderResult_Vtbl,
    0x6f7fd316_fe29_59d1_9343_c49cf5cde680
);
impl windows_core::RuntimeType for IPickFolderResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPickFolderResult {
    pub fn Path(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Path)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
}
#[repr(C)]
pub struct IPickFolderResult_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Path: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPointer,
    IPointer_Vtbl,
    0x1f9afbf5_11a3_5e68_aa1b_72febfa0ab23
);
impl windows_core::RuntimeType for IPointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IPointer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IPointerPoint,
    IPointerPoint_Vtbl,
    0x0d430ee6_252c_59a4_b2a2_d44264dc6a40
);
impl windows_core::RuntimeType for IPointerPoint {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPointerPoint {
    pub fn Position(&self) -> windows_core::Result<Point> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Position)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Properties(&self) -> windows_core::Result<PointerPointProperties> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Properties)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IPointerPoint_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FrameId: usize,
    IsInContact: usize,
    PointerDeviceType: usize,
    PointerId: usize,
    pub Position:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Point) -> windows_core::HRESULT,
    pub Properties: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPointerPointProperties,
    IPointerPointProperties_Vtbl,
    0xd760ed77_4b10_57a5_b3cc_d9bf3413e996
);
impl windows_core::RuntimeType for IPointerPointProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPointerPointProperties {
    pub fn PointerUpdateKind(&self) -> windows_core::Result<PointerUpdateKind> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PointerUpdateKind)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IPointerPointProperties_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ContactRect: usize,
    IsBarrelButtonPressed: usize,
    IsCanceled: usize,
    IsEraser: usize,
    IsHorizontalMouseWheel: usize,
    IsInRange: usize,
    IsInverted: usize,
    IsLeftButtonPressed: usize,
    IsMiddleButtonPressed: usize,
    IsPrimary: usize,
    IsRightButtonPressed: usize,
    IsXButton1Pressed: usize,
    IsXButton2Pressed: usize,
    MouseWheelDelta: usize,
    Orientation: usize,
    pub PointerUpdateKind: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut PointerUpdateKind,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IPointerRoutedEventArgs,
    IPointerRoutedEventArgs_Vtbl,
    0x66e78a9a_1bec_5f92_b1a1_ea6334ee511c
);
impl windows_core::RuntimeType for IPointerRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPointerRoutedEventArgs {
    pub fn Pointer(&self) -> windows_core::Result<Pointer> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Pointer)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetHandled(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetHandled)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn GetCurrentPoint<P0>(&self, relativeto: P0) -> windows_core::Result<PointerPoint>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetCurrentPoint)(
                windows_core::Interface::as_raw(self),
                relativeto.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IPointerRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Pointer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    KeyModifiers: usize,
    Handled: usize,
    pub SetHandled:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    IsGenerated: usize,
    pub GetCurrentPoint: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(IPopup, IPopup_Vtbl, 0x4e3ab19d_2f95_579c_9535_906c58629437);
impl windows_core::RuntimeType for IPopup {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IPopup {
    pub fn Child(&self) -> windows_core::Result<UIElement> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Child)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IPopup_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Child: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IProgressBar,
    IProgressBar_Vtbl,
    0x87555c8c_0aaf_52c1_8390_0db17f40438e
);
impl windows_core::RuntimeType for IProgressBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IProgressBar {
    pub fn SetIsIndeterminate(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsIndeterminate)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IProgressBar_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsIndeterminate: usize,
    pub SetIsIndeterminate:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IProgressRing,
    IProgressRing_Vtbl,
    0x2670d03f_e28c_5652_bee2_b5212ebdf7ff
);
impl windows_core::RuntimeType for IProgressRing {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IProgressRing {
    pub fn SetIsActive(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsActive)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IProgressRing_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    IsActive: usize,
    pub SetIsActive:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRandomAccessStream,
    IRandomAccessStream_Vtbl,
    0x905a0fe1_bc53_11df_8c49_001e4fc686da
);
impl windows_core::RuntimeType for IRandomAccessStream {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IRandomAccessStream,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IRandomAccessStream {
    pub fn Size(&self) -> windows_core::Result<u64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Size)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn GetInputStreamAt(&self, position: u64) -> windows_core::Result<IInputStream> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetInputStreamAt)(
                windows_core::Interface::as_raw(self),
                position,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetOutputStreamAt(&self, position: u64) -> windows_core::Result<IOutputStream> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetOutputStreamAt)(
                windows_core::Interface::as_raw(self),
                position,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IRandomAccessStream_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Size: unsafe extern "system" fn(*mut core::ffi::c_void, *mut u64) -> windows_core::HRESULT,
    SetSize: usize,
    pub GetInputStreamAt: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u64,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetOutputStreamAt: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        u64,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRangeBase,
    IRangeBase_Vtbl,
    0x540d6d61_8fac_5d5c_b5b0_e172a7dde103
);
impl windows_core::RuntimeType for IRangeBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IRangeBase {
    pub fn SetMaximum(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetMaximum)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetValue(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetValue)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IRangeBase_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Minimum: usize,
    SetMinimum: usize,
    Maximum: usize,
    pub SetMaximum: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    SmallChange: usize,
    SetSmallChange: usize,
    LargeChange: usize,
    SetLargeChange: usize,
    Value: usize,
    pub SetValue: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IResourceDictionary,
    IResourceDictionary_Vtbl,
    0x1b690975_a710_5783_a6e1_15836f6186c2
);
impl windows_core::RuntimeType for IResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IResourceDictionary {
    pub fn MergedDictionaries(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<ResourceDictionary>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).MergedDictionaries)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IResourceDictionary_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Source: usize,
    SetSource: usize,
    pub MergedDictionaries: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IRoutedEventArgs,
    IRoutedEventArgs_Vtbl,
    0x0908c407_1c7d_5de3_9c50_d971c62ec8ec
);
impl windows_core::RuntimeType for IRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IRoutedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IScrollViewer,
    IScrollViewer_Vtbl,
    0x1dc28c2e_996c_5394_89c3_4dc656b4ad46
);
impl windows_core::RuntimeType for IScrollViewer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IScrollViewer {
    pub fn HorizontalOffset(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).HorizontalOffset)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ScrollableWidth(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ScrollableWidth)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn VerticalOffset(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).VerticalOffset)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ViewportHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ViewportHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ScrollableHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ScrollableHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ExtentHeight(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ExtentHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ChangeViewWithOptionalAnimation(
        &self,
        horizontaloffset: Option<f64>,
        verticaloffset: Option<f64>,
        zoomfactor: Option<f32>,
        disableanimation: bool,
    ) -> windows_core::Result<bool> {
        let horizontaloffset__ =
            horizontaloffset.map(<windows_reference::IReference<f64> as From<_>>::from);
        let verticaloffset__ =
            verticaloffset.map(<windows_reference::IReference<f64> as From<_>>::from);
        let zoomfactor__ = zoomfactor.map(<windows_reference::IReference<f32> as From<_>>::from);
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ChangeViewWithOptionalAnimation)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(horizontaloffset__.as_ref()).abi(),
                windows_core::Param::param(verticaloffset__.as_ref()).abi(),
                windows_core::Param::param(zoomfactor__.as_ref()).abi(),
                disableanimation,
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IScrollViewer_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    HorizontalScrollBarVisibility: usize,
    SetHorizontalScrollBarVisibility: usize,
    VerticalScrollBarVisibility: usize,
    SetVerticalScrollBarVisibility: usize,
    IsHorizontalRailEnabled: usize,
    SetIsHorizontalRailEnabled: usize,
    IsVerticalRailEnabled: usize,
    SetIsVerticalRailEnabled: usize,
    IsHorizontalScrollChainingEnabled: usize,
    SetIsHorizontalScrollChainingEnabled: usize,
    IsVerticalScrollChainingEnabled: usize,
    SetIsVerticalScrollChainingEnabled: usize,
    IsZoomChainingEnabled: usize,
    SetIsZoomChainingEnabled: usize,
    IsScrollInertiaEnabled: usize,
    SetIsScrollInertiaEnabled: usize,
    IsZoomInertiaEnabled: usize,
    SetIsZoomInertiaEnabled: usize,
    HorizontalScrollMode: usize,
    SetHorizontalScrollMode: usize,
    VerticalScrollMode: usize,
    SetVerticalScrollMode: usize,
    ZoomMode: usize,
    SetZoomMode: usize,
    HorizontalSnapPointsAlignment: usize,
    SetHorizontalSnapPointsAlignment: usize,
    VerticalSnapPointsAlignment: usize,
    SetVerticalSnapPointsAlignment: usize,
    HorizontalSnapPointsType: usize,
    SetHorizontalSnapPointsType: usize,
    VerticalSnapPointsType: usize,
    SetVerticalSnapPointsType: usize,
    ZoomSnapPointsType: usize,
    SetZoomSnapPointsType: usize,
    pub HorizontalOffset:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    ViewportWidth: usize,
    pub ScrollableWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    ComputedHorizontalScrollBarVisibility: usize,
    ExtentWidth: usize,
    pub VerticalOffset:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub ViewportHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub ScrollableHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    ComputedVerticalScrollBarVisibility: usize,
    pub ExtentHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    MinZoomFactor: usize,
    SetMinZoomFactor: usize,
    MaxZoomFactor: usize,
    SetMaxZoomFactor: usize,
    ZoomFactor: usize,
    ZoomSnapPoints: usize,
    TopLeftHeader: usize,
    SetTopLeftHeader: usize,
    LeftHeader: usize,
    SetLeftHeader: usize,
    TopHeader: usize,
    SetTopHeader: usize,
    ReduceViewportForCoreInputViewOcclusions: usize,
    SetReduceViewportForCoreInputViewOcclusions: usize,
    HorizontalAnchorRatio: usize,
    SetHorizontalAnchorRatio: usize,
    VerticalAnchorRatio: usize,
    SetVerticalAnchorRatio: usize,
    CanContentRenderOutsideBounds: usize,
    SetCanContentRenderOutsideBounds: usize,
    AnchorRequested: usize,
    RemoveAnchorRequested: usize,
    ViewChanging: usize,
    RemoveViewChanging: usize,
    ViewChanged: usize,
    RemoveViewChanged: usize,
    DirectManipulationStarted: usize,
    RemoveDirectManipulationStarted: usize,
    DirectManipulationCompleted: usize,
    RemoveDirectManipulationCompleted: usize,
    ScrollToHorizontalOffset: usize,
    ScrollToVerticalOffset: usize,
    ZoomToFactor: usize,
    ChangeView: usize,
    pub ChangeViewWithOptionalAnimation: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        bool,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISelectionChangedEventArgs,
    ISelectionChangedEventArgs_Vtbl,
    0xb6c18076_4b76_5416_ad29_e2dc20c46246
);
impl windows_core::RuntimeType for ISelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISelectionChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISelector,
    ISelector_Vtbl,
    0x8f7e2159_e61d_576f_8476_f83fde3d689e
);
impl windows_core::RuntimeType for ISelector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ISelector {
    pub fn SelectedIndex(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedIndex)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetSelectedIndex(&self, value: i32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedIndex)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SelectedItem(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedItem)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetSelectedItem<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedItem)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SelectionChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<SelectionChangedEventArgs>,
            ) + 'static,
    {
        let handler: SelectionChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::new(
                &SelectionChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SelectionChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSelectionChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ISelector_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    pub SetSelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    pub SelectedItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetSelectedItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SelectedValue: usize,
    SetSelectedValue: usize,
    SelectedValuePath: usize,
    SetSelectedValuePath: usize,
    IsSynchronizedWithCurrentItem: usize,
    SetIsSynchronizedWithCurrentItem: usize,
    pub SelectionChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSelectionChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISelectorItem,
    ISelectorItem_Vtbl,
    0x5772c4de_60ea_5492_8c5e_b3323d5a3ca6
);
impl windows_core::RuntimeType for ISelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISelectorItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISizeChangedEventArgs,
    ISizeChangedEventArgs_Vtbl,
    0xfe76324e_6dfb_58b1_9dcd_886ca8f9a2ea
);
impl windows_core::RuntimeType for ISizeChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISizeChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ISoftwareBitmap,
    ISoftwareBitmap_Vtbl,
    0x689e0708_7eef_483f_963f_da938818e073
);
impl windows_core::RuntimeType for ISoftwareBitmap {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ISoftwareBitmap {
    pub fn PixelWidth(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PixelWidth)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn PixelHeight(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).PixelHeight)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn CopyToBuffer<P0>(&self, buffer: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<IBuffer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).CopyToBuffer)(
                windows_core::Interface::as_raw(self),
                buffer.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ISoftwareBitmap_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    BitmapPixelFormat: usize,
    BitmapAlphaMode: usize,
    pub PixelWidth:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    pub PixelHeight:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    IsReadOnly: usize,
    SetDpiX: usize,
    DpiX: usize,
    SetDpiY: usize,
    DpiY: usize,
    LockBuffer: usize,
    CopyTo: usize,
    CopyFromBuffer: usize,
    pub CopyToBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISoftwareBitmapStatics,
    ISoftwareBitmapStatics_Vtbl,
    0xdf0385db_672f_4a9d_806e_c2442f343e86
);
impl windows_core::RuntimeType for ISoftwareBitmapStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISoftwareBitmapStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Copy: usize,
    Convert: usize,
    ConvertWithAlpha: usize,
    CreateCopyFromBuffer: usize,
    pub CreateCopyWithAlphaFromBuffer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        BitmapPixelFormat,
        i32,
        i32,
        BitmapAlphaMode,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub CreateCopyFromSurfaceAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IStoryboard,
    IStoryboard_Vtbl,
    0x04d41bb3_8721_519e_8e53_fb8b34920305
);
impl windows_core::RuntimeType for IStoryboard {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IStoryboard {
    pub fn Stop(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Stop)(windows_core::Interface::as_raw(self)).ok()
        }
    }
    pub fn Begin(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Begin)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IStoryboard_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Children: usize,
    Seek: usize,
    pub Stop: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Begin: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ISystemBackdrop,
    ISystemBackdrop_Vtbl,
    0x5aeed5c4_37ac_5852_b73f_1b76ebc3205f
);
impl windows_core::RuntimeType for ISystemBackdrop {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ISystemBackdrop_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITabView,
    ITabView_Vtbl,
    0x07b509e1_1d38_551b_95f4_4732b049f6a6
);
impl windows_core::RuntimeType for ITabView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITabView {
    pub fn TabCloseRequested<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<TabView>, windows_core::Ref<TabViewTabCloseRequestedEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<TabView, TabViewTabCloseRequestedEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<TabView, TabViewTabCloseRequestedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<TabView, TabViewTabCloseRequestedEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TabCloseRequested)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTabCloseRequested,
            ))
        }
    }
    pub fn AddTabButtonClick<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<TabView>, windows_core::Ref<windows_core::IInspectable>) + 'static,
    {
        let handler: TypedEventHandler<TabView, windows_core::IInspectable> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<TabView, windows_core::IInspectable>,
                F,
            >::new(
                &TypedEventHandlerBox::<TabView, windows_core::IInspectable, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).AddTabButtonClick)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveAddTabButtonClick,
            ))
        }
    }
    pub fn TabItems(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TabItems)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SelectedIndex(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedIndex)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetSelectedIndex(&self, value: i32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedIndex)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SelectedItem(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedItem)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetSelectedItem<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedItem)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SelectionChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<SelectionChangedEventArgs>,
            ) + 'static,
    {
        let handler: SelectionChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::new(
                &SelectionChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SelectionChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSelectionChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ITabView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    TabWidthMode: usize,
    SetTabWidthMode: usize,
    CloseButtonOverlayMode: usize,
    SetCloseButtonOverlayMode: usize,
    TabStripHeader: usize,
    SetTabStripHeader: usize,
    TabStripHeaderTemplate: usize,
    SetTabStripHeaderTemplate: usize,
    TabStripFooter: usize,
    SetTabStripFooter: usize,
    TabStripFooterTemplate: usize,
    SetTabStripFooterTemplate: usize,
    IsAddTabButtonVisible: usize,
    SetIsAddTabButtonVisible: usize,
    AddTabButtonCommand: usize,
    SetAddTabButtonCommand: usize,
    AddTabButtonCommandParameter: usize,
    SetAddTabButtonCommandParameter: usize,
    pub TabCloseRequested: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTabCloseRequested:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    TabDroppedOutside: usize,
    RemoveTabDroppedOutside: usize,
    pub AddTabButtonClick: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveAddTabButtonClick:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    TabItemsChanged: usize,
    RemoveTabItemsChanged: usize,
    TabItemsSource: usize,
    SetTabItemsSource: usize,
    pub TabItems: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    TabItemTemplate: usize,
    SetTabItemTemplate: usize,
    TabItemTemplateSelector: usize,
    SetTabItemTemplateSelector: usize,
    CanDragTabs: usize,
    SetCanDragTabs: usize,
    CanReorderTabs: usize,
    SetCanReorderTabs: usize,
    AllowDropTabs: usize,
    SetAllowDropTabs: usize,
    pub SelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    pub SetSelectedIndex:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32) -> windows_core::HRESULT,
    pub SelectedItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetSelectedItem: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ContainerFromItem: usize,
    ContainerFromIndex: usize,
    pub SelectionChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSelectionChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabViewItem,
    ITabViewItem_Vtbl,
    0x64980afa_97af_5190_90b3_4ba277b1113d
);
impl windows_core::RuntimeType for ITabViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITabViewItem {
    pub fn SetHeader<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetHeader)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetIsClosable(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsClosable)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ITabViewItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Header: usize,
    pub SetHeader: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    HeaderTemplate: usize,
    SetHeaderTemplate: usize,
    IconSource: usize,
    SetIconSource: usize,
    IsClosable: usize,
    pub SetIsClosable:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabViewItemFactory,
    ITabViewItemFactory_Vtbl,
    0xb64c2423_7e56_5d41_8a84_1ee28f9826a4
);
impl windows_core::RuntimeType for ITabViewItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITabViewItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITabViewTabCloseRequestedEventArgs,
    ITabViewTabCloseRequestedEventArgs_Vtbl,
    0xd56ab9b2_e264_5c7e_a1cb_e41a16a6c6c6
);
impl windows_core::RuntimeType for ITabViewTabCloseRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITabViewTabCloseRequestedEventArgs {
    pub fn Tab(&self) -> windows_core::Result<TabViewItem> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Tab)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ITabViewTabCloseRequestedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Item: usize,
    pub Tab: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBlock,
    ITextBlock_Vtbl,
    0x1ac8d84f_392c_5c7e_83f5_a53e3bf0abb0
);
impl windows_core::RuntimeType for ITextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITextBlock {
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ITextBlock_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FontSize: usize,
    SetFontSize: usize,
    FontFamily: usize,
    SetFontFamily: usize,
    FontWeight: usize,
    SetFontWeight: usize,
    FontStyle: usize,
    SetFontStyle: usize,
    FontStretch: usize,
    SetFontStretch: usize,
    CharacterSpacing: usize,
    SetCharacterSpacing: usize,
    Foreground: usize,
    SetForeground: usize,
    TextWrapping: usize,
    SetTextWrapping: usize,
    TextTrimming: usize,
    SetTextTrimming: usize,
    TextAlignment: usize,
    SetTextAlignment: usize,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextBox,
    ITextBox_Vtbl,
    0x873af7c2_ab89_5d76_8dbe_3d6325669df5
);
impl windows_core::RuntimeType for ITextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITextBox {
    pub fn Text(&self) -> windows_core::Result<String> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Text)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| {
                let hstring: windows_core::HSTRING = core::mem::transmute(result__);
                hstring.to_string_lossy()
            })
        }
    }
    pub fn SetText(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetText)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SelectionLength(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectionLength)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SelectionStart(&self) -> windows_core::Result<i32> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectionStart)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetTextAlignment(&self, value: TextAlignment) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTextAlignment)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn TextChanged<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<TextChangedEventArgs>,
            ) + 'static,
    {
        let handler: TextChangedEventHandler = {
            let com = windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::new(
                &TextChangedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).TextChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveTextChanged,
            ))
        }
    }
    pub fn Select(&self, start: i32, length: i32) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Select)(
                windows_core::Interface::as_raw(self),
                start,
                length,
            )
            .ok()
        }
    }
    pub fn SelectAll(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SelectAll)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct ITextBox_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Text: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetText: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    SelectedText: usize,
    SetSelectedText: usize,
    pub SelectionLength:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    SetSelectionLength: usize,
    pub SelectionStart:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut i32) -> windows_core::HRESULT,
    SetSelectionStart: usize,
    MaxLength: usize,
    SetMaxLength: usize,
    IsReadOnly: usize,
    SetIsReadOnly: usize,
    AcceptsReturn: usize,
    SetAcceptsReturn: usize,
    TextAlignment: usize,
    pub SetTextAlignment:
        unsafe extern "system" fn(*mut core::ffi::c_void, TextAlignment) -> windows_core::HRESULT,
    TextWrapping: usize,
    SetTextWrapping: usize,
    IsSpellCheckEnabled: usize,
    SetIsSpellCheckEnabled: usize,
    IsTextPredictionEnabled: usize,
    SetIsTextPredictionEnabled: usize,
    InputScope: usize,
    SetInputScope: usize,
    Header: usize,
    SetHeader: usize,
    HeaderTemplate: usize,
    SetHeaderTemplate: usize,
    PlaceholderText: usize,
    SetPlaceholderText: usize,
    SelectionHighlightColor: usize,
    SetSelectionHighlightColor: usize,
    PreventKeyboardDisplayOnProgrammaticFocus: usize,
    SetPreventKeyboardDisplayOnProgrammaticFocus: usize,
    IsColorFontEnabled: usize,
    SetIsColorFontEnabled: usize,
    SelectionHighlightColorWhenNotFocused: usize,
    SetSelectionHighlightColorWhenNotFocused: usize,
    HorizontalTextAlignment: usize,
    SetHorizontalTextAlignment: usize,
    CharacterCasing: usize,
    SetCharacterCasing: usize,
    PlaceholderForeground: usize,
    SetPlaceholderForeground: usize,
    CanPasteClipboardContent: usize,
    CanUndo: usize,
    CanRedo: usize,
    SelectionFlyout: usize,
    SetSelectionFlyout: usize,
    ProofingMenuFlyout: usize,
    Description: usize,
    SetDescription: usize,
    pub TextChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveTextChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    SelectionChanged: usize,
    RemoveSelectionChanged: usize,
    ContextMenuOpening: usize,
    RemoveContextMenuOpening: usize,
    Paste: usize,
    RemovePaste: usize,
    TextCompositionStarted: usize,
    RemoveTextCompositionStarted: usize,
    TextCompositionChanged: usize,
    RemoveTextCompositionChanged: usize,
    TextCompositionEnded: usize,
    RemoveTextCompositionEnded: usize,
    CopyingToClipboard: usize,
    RemoveCopyingToClipboard: usize,
    CuttingToClipboard: usize,
    RemoveCuttingToClipboard: usize,
    BeforeTextChanging: usize,
    RemoveBeforeTextChanging: usize,
    SelectionChanging: usize,
    RemoveSelectionChanging: usize,
    pub Select:
        unsafe extern "system" fn(*mut core::ffi::c_void, i32, i32) -> windows_core::HRESULT,
    pub SelectAll: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITextChangedEventArgs,
    ITextChangedEventArgs_Vtbl,
    0x71c37e43_7be7_52fc_bf8c_9867f44be5f4
);
impl windows_core::RuntimeType for ITextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITextChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITimeline,
    ITimeline_Vtbl,
    0xd0f9b330_cc2a_5b05_9786_2da4c6584581
);
impl windows_core::RuntimeType for ITimeline {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITimeline_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IToggleButton,
    IToggleButton_Vtbl,
    0x686fbaa4_c866_568b_8f75_481d8d545291
);
impl windows_core::RuntimeType for IToggleButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IToggleButton {
    pub fn IsChecked(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsChecked)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
            .and_then(|r__: windows_reference::IReference<bool>| r__.Value())
        }
    }
    pub fn SetIsChecked(&self, value: Option<bool>) -> windows_core::Result<()> {
        let value__ = value.map(<windows_reference::IReference<bool> as From<_>>::from);
        unsafe {
            (windows_core::Interface::vtable(self).SetIsChecked)(
                windows_core::Interface::as_raw(self),
                windows_core::Param::param(value__.as_ref()).abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IToggleButton_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsChecked: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetIsChecked: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IToggleMenuFlyoutItem,
    IToggleMenuFlyoutItem_Vtbl,
    0x1803f260_67e4_5bc1_a63a_123510167bb8
);
impl windows_core::RuntimeType for IToggleMenuFlyoutItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IToggleMenuFlyoutItem {
    pub fn IsChecked(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsChecked)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsChecked(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsChecked)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IToggleMenuFlyoutItem_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsChecked:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsChecked:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IToggleMenuFlyoutItemFactory,
    IToggleMenuFlyoutItemFactory_Vtbl,
    0x426dfd57_6cc9_570f_950d_37437235dc89
);
impl windows_core::RuntimeType for IToggleMenuFlyoutItemFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IToggleMenuFlyoutItemFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IToggleSwitch,
    IToggleSwitch_Vtbl,
    0x1b17eeb1_74bf_5a83_8161_a86f0fdcdf24
);
impl windows_core::RuntimeType for IToggleSwitch {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IToggleSwitch {
    pub fn IsOn(&self) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).IsOn)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetIsOn(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsOn)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Toggled<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Toggled)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveToggled,
            ))
        }
    }
}
#[repr(C)]
pub struct IToggleSwitch_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub IsOn: unsafe extern "system" fn(*mut core::ffi::c_void, *mut bool) -> windows_core::HRESULT,
    pub SetIsOn: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Header: usize,
    SetHeader: usize,
    HeaderTemplate: usize,
    SetHeaderTemplate: usize,
    OnContent: usize,
    SetOnContent: usize,
    OnContentTemplate: usize,
    SetOnContentTemplate: usize,
    OffContent: usize,
    SetOffContent: usize,
    OffContentTemplate: usize,
    SetOffContentTemplate: usize,
    TemplateSettings: usize,
    pub Toggled: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveToggled:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IToolTipService,
    IToolTipService_Vtbl,
    0x01140768_2727_5f89_80e0_5210326a3431
);
impl windows_core::RuntimeType for IToolTipService {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IToolTipService_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IToolTipServiceStatics,
    IToolTipServiceStatics_Vtbl,
    0x5aa38adc_9874_5e0a_8d8e_1574efc0b88f
);
impl windows_core::RuntimeType for IToolTipServiceStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IToolTipServiceStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    PlacementProperty: usize,
    GetPlacement: usize,
    SetPlacement: usize,
    PlacementTargetProperty: usize,
    GetPlacementTarget: usize,
    SetPlacementTarget: usize,
    ToolTipProperty: usize,
    GetToolTip: usize,
    pub SetToolTip: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeView,
    ITreeView_Vtbl,
    0x1bef9af4_712c_50ef_9bb4_881b975232ab
);
impl windows_core::RuntimeType for ITreeView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITreeView {
    pub fn RootNodes(&self) -> windows_core::Result<windows_collections::IVector<TreeViewNode>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RootNodes)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ITreeView_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub RootNodes: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeView2,
    ITreeView2_Vtbl,
    0xb947ca7d_0f6f_594c_83ec_14153d343225
);
impl windows_core::RuntimeType for ITreeView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITreeView2 {
    pub fn DragItemsStarting<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<TreeView>, windows_core::Ref<TreeViewDragItemsStartingEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<TreeView, TreeViewDragItemsStartingEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<TreeView, TreeViewDragItemsStartingEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<TreeView, TreeViewDragItemsStartingEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DragItemsStarting)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDragItemsStarting,
            ))
        }
    }
    pub fn DragItemsCompleted<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<TreeView>, windows_core::Ref<TreeViewDragItemsCompletedEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<TreeView, TreeViewDragItemsCompletedEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<TreeView, TreeViewDragItemsCompletedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<TreeView, TreeViewDragItemsCompletedEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).DragItemsCompleted)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveDragItemsCompleted,
            ))
        }
    }
    pub fn SelectedNode(&self) -> windows_core::Result<TreeViewNode> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).SelectedNode)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetSelectedNode<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<TreeViewNode>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSelectedNode)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct ITreeView2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    NodeFromContainer: usize,
    ContainerFromNode: usize,
    ItemFromContainer: usize,
    ContainerFromItem: usize,
    CanDragItems: usize,
    SetCanDragItems: usize,
    CanReorderItems: usize,
    SetCanReorderItems: usize,
    ItemTemplate: usize,
    SetItemTemplate: usize,
    ItemTemplateSelector: usize,
    SetItemTemplateSelector: usize,
    ItemContainerStyle: usize,
    SetItemContainerStyle: usize,
    ItemContainerStyleSelector: usize,
    SetItemContainerStyleSelector: usize,
    ItemContainerTransitions: usize,
    SetItemContainerTransitions: usize,
    ItemsSource: usize,
    SetItemsSource: usize,
    pub DragItemsStarting: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsStarting:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub DragItemsCompleted: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveDragItemsCompleted:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub SelectedNode: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetSelectedNode: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeView3,
    ITreeView3_Vtbl,
    0xa1b5538e_7956_5671_afd0_4c0f38122b70
);
impl windows_core::RuntimeType for ITreeView3 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITreeView3 {
    pub fn SelectionChanged<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<TreeView>, windows_core::Ref<TreeViewSelectionChangedEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<TreeView, TreeViewSelectionChangedEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<TreeView, TreeViewSelectionChangedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<TreeView, TreeViewSelectionChangedEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).SelectionChanged)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveSelectionChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct ITreeView3_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub SelectionChanged: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveSelectionChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewDragItemsCompletedEventArgs,
    ITreeViewDragItemsCompletedEventArgs_Vtbl,
    0xe5b8547e_f839_55db_9c26_2a95f57a60dc
);
impl windows_core::RuntimeType for ITreeViewDragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITreeViewDragItemsCompletedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    ITreeViewDragItemsStartingEventArgs,
    ITreeViewDragItemsStartingEventArgs_Vtbl,
    0x1b6c4ffc_cd32_5e06_b782_df9f077546c7
);
impl windows_core::RuntimeType for ITreeViewDragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITreeViewDragItemsStartingEventArgs {
    pub fn SetCancel(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetCancel)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Items(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<windows_core::IInspectable>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Items)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ITreeViewDragItemsStartingEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Cancel: usize,
    pub SetCancel: unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Data: usize,
    pub Items: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewNode,
    ITreeViewNode_Vtbl,
    0x00378a74_790b_5328_8afa_7d65e22da426
);
impl windows_core::RuntimeType for ITreeViewNode {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl ITreeViewNode {
    pub fn Content(&self) -> windows_core::Result<windows_core::IInspectable> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Content)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<windows_core::IInspectable>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn Parent(&self) -> windows_core::Result<TreeViewNode> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Parent)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetIsExpanded(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsExpanded)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Children(&self) -> windows_core::Result<windows_collections::IVector<TreeViewNode>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Children)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct ITreeViewNode_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Content: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub Parent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    IsExpanded: usize,
    pub SetIsExpanded:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    HasChildren: usize,
    Depth: usize,
    HasUnrealizedChildren: usize,
    SetHasUnrealizedChildren: usize,
    pub Children: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewNodeFactory,
    ITreeViewNodeFactory_Vtbl,
    0xc105a5e5_cea8_5efd_8be8_3d89b54cbd5f
);
impl windows_core::RuntimeType for ITreeViewNodeFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITreeViewNodeFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    ITreeViewSelectionChangedEventArgs,
    ITreeViewSelectionChangedEventArgs_Vtbl,
    0x664190f3_7133_5599_b41c_1d54cd2cb930
);
impl windows_core::RuntimeType for ITreeViewSelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ITreeViewSelectionChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IUIElement,
    IUIElement_Vtbl,
    0xc3c01020_320c_5cf6_9d24_d396bbfa4d8b
);
impl windows_core::RuntimeType for IUIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IUIElement {
    pub fn DesiredSize(&self) -> windows_core::Result<Size> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).DesiredSize)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Opacity(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Opacity)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetOpacity(&self, value: f64) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetOpacity)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetIsHitTestVisible(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetIsHitTestVisible)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Visibility(&self) -> windows_core::Result<Visibility> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Visibility)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn SetVisibility(&self, value: Visibility) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetVisibility)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn ContextFlyout(&self) -> windows_core::Result<FlyoutBase> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).ContextFlyout)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetContextFlyout<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<FlyoutBase>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContextFlyout)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn KeyboardAccelerators(
        &self,
    ) -> windows_core::Result<windows_collections::IVector<KeyboardAccelerator>> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).KeyboardAccelerators)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetKeyboardAcceleratorPlacementMode(
        &self,
        value: KeyboardAcceleratorPlacementMode,
    ) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetKeyboardAcceleratorPlacementMode)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn SetTranslation(&self, value: Vector3) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTranslation)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn XamlRoot(&self) -> windows_core::Result<XamlRoot> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).XamlRoot)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn SetXamlRoot<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<XamlRoot>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetXamlRoot)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn KeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + 'static,
    {
        let handler: KeyEventHandler = {
            let com = windows_core::imp::DelegateBox::<KeyEventHandler, F>::new(
                &KeyEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).KeyDown)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveKeyDown,
            ))
        }
    }
    pub fn GotFocus<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).GotFocus)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveGotFocus,
            ))
        }
    }
    pub fn LostFocus<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>)
            + 'static,
    {
        let handler: RoutedEventHandler = {
            let com = windows_core::imp::DelegateBox::<RoutedEventHandler, F>::new(
                &RoutedEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).LostFocus)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveLostFocus,
            ))
        }
    }
    pub fn PointerPressed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerPressed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerPressed,
            ))
        }
    }
    pub fn PointerMoved<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerMoved)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerMoved,
            ))
        }
    }
    pub fn PointerReleased<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerReleased)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerReleased,
            ))
        }
    }
    pub fn PointerEntered<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerEntered)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerEntered,
            ))
        }
    }
    pub fn PointerExited<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerExited)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerExited,
            ))
        }
    }
    pub fn PointerCaptureLost<F>(
        &self,
        handler: F,
    ) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(
                windows_core::Ref<windows_core::IInspectable>,
                windows_core::Ref<PointerRoutedEventArgs>,
            ) + 'static,
    {
        let handler: PointerEventHandler = {
            let com = windows_core::imp::DelegateBox::<PointerEventHandler, F>::new(
                &PointerEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PointerCaptureLost)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePointerCaptureLost,
            ))
        }
    }
    pub fn PreviewKeyDown<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
            + 'static,
    {
        let handler: KeyEventHandler = {
            let com = windows_core::imp::DelegateBox::<KeyEventHandler, F>::new(
                &KeyEventHandlerBox::<F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).PreviewKeyDown)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemovePreviewKeyDown,
            ))
        }
    }
    pub fn Measure(&self, availablesize: Size) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Measure)(
                windows_core::Interface::as_raw(self),
                availablesize,
            )
            .ok()
        }
    }
    pub fn CapturePointer<P0>(&self, value: P0) -> windows_core::Result<bool>
    where
        P0: windows_core::Param<Pointer>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CapturePointer)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn ReleasePointerCapture<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<Pointer>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).ReleasePointerCapture)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn TransformToVisual<P0>(&self, visual: P0) -> windows_core::Result<GeneralTransform>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).TransformToVisual)(
                windows_core::Interface::as_raw(self),
                visual.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn UpdateLayout(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).UpdateLayout)(windows_core::Interface::as_raw(
                self,
            ))
            .ok()
        }
    }
    pub fn Focus(&self, value: FocusState) -> windows_core::Result<bool> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).Focus)(
                windows_core::Interface::as_raw(self),
                value,
                &mut result__,
            )
            .map(|| result__)
        }
    }
}
#[repr(C)]
pub struct IUIElement_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub DesiredSize:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Size) -> windows_core::HRESULT,
    AllowDrop: usize,
    SetAllowDrop: usize,
    pub Opacity:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    pub SetOpacity: unsafe extern "system" fn(*mut core::ffi::c_void, f64) -> windows_core::HRESULT,
    Clip: usize,
    SetClip: usize,
    RenderTransform: usize,
    SetRenderTransform: usize,
    Projection: usize,
    SetProjection: usize,
    Transform3D: usize,
    SetTransform3D: usize,
    RenderTransformOrigin: usize,
    SetRenderTransformOrigin: usize,
    IsHitTestVisible: usize,
    pub SetIsHitTestVisible:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    pub Visibility:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut Visibility) -> windows_core::HRESULT,
    pub SetVisibility:
        unsafe extern "system" fn(*mut core::ffi::c_void, Visibility) -> windows_core::HRESULT,
    RenderSize: usize,
    UseLayoutRounding: usize,
    SetUseLayoutRounding: usize,
    Transitions: usize,
    SetTransitions: usize,
    CacheMode: usize,
    SetCacheMode: usize,
    IsTapEnabled: usize,
    SetIsTapEnabled: usize,
    IsDoubleTapEnabled: usize,
    SetIsDoubleTapEnabled: usize,
    CanDrag: usize,
    SetCanDrag: usize,
    IsRightTapEnabled: usize,
    SetIsRightTapEnabled: usize,
    IsHoldingEnabled: usize,
    SetIsHoldingEnabled: usize,
    ManipulationMode: usize,
    SetManipulationMode: usize,
    PointerCaptures: usize,
    pub ContextFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetContextFlyout: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CompositeMode: usize,
    SetCompositeMode: usize,
    Lights: usize,
    CanBeScrollAnchor: usize,
    SetCanBeScrollAnchor: usize,
    ExitDisplayModeOnAccessKeyInvoked: usize,
    SetExitDisplayModeOnAccessKeyInvoked: usize,
    IsAccessKeyScope: usize,
    SetIsAccessKeyScope: usize,
    AccessKeyScopeOwner: usize,
    SetAccessKeyScopeOwner: usize,
    AccessKey: usize,
    SetAccessKey: usize,
    KeyTipPlacementMode: usize,
    SetKeyTipPlacementMode: usize,
    KeyTipHorizontalOffset: usize,
    SetKeyTipHorizontalOffset: usize,
    KeyTipVerticalOffset: usize,
    SetKeyTipVerticalOffset: usize,
    KeyTipTarget: usize,
    SetKeyTipTarget: usize,
    XYFocusKeyboardNavigation: usize,
    SetXYFocusKeyboardNavigation: usize,
    XYFocusUpNavigationStrategy: usize,
    SetXYFocusUpNavigationStrategy: usize,
    XYFocusDownNavigationStrategy: usize,
    SetXYFocusDownNavigationStrategy: usize,
    XYFocusLeftNavigationStrategy: usize,
    SetXYFocusLeftNavigationStrategy: usize,
    XYFocusRightNavigationStrategy: usize,
    SetXYFocusRightNavigationStrategy: usize,
    pub KeyboardAccelerators: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    KeyboardAcceleratorPlacementTarget: usize,
    SetKeyboardAcceleratorPlacementTarget: usize,
    KeyboardAcceleratorPlacementMode: usize,
    pub SetKeyboardAcceleratorPlacementMode: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        KeyboardAcceleratorPlacementMode,
    )
        -> windows_core::HRESULT,
    HighContrastAdjustment: usize,
    SetHighContrastAdjustment: usize,
    TabFocusNavigation: usize,
    SetTabFocusNavigation: usize,
    OpacityTransition: usize,
    SetOpacityTransition: usize,
    Translation: usize,
    pub SetTranslation:
        unsafe extern "system" fn(*mut core::ffi::c_void, Vector3) -> windows_core::HRESULT,
    TranslationTransition: usize,
    SetTranslationTransition: usize,
    Rotation: usize,
    SetRotation: usize,
    RotationTransition: usize,
    SetRotationTransition: usize,
    Scale: usize,
    SetScale: usize,
    ScaleTransition: usize,
    SetScaleTransition: usize,
    TransformMatrix: usize,
    SetTransformMatrix: usize,
    CenterPoint: usize,
    SetCenterPoint: usize,
    RotationAxis: usize,
    SetRotationAxis: usize,
    ActualOffset: usize,
    ActualSize: usize,
    pub XamlRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub SetXamlRoot: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    Shadow: usize,
    SetShadow: usize,
    RasterizationScale: usize,
    SetRasterizationScale: usize,
    FocusState: usize,
    UseSystemFocusVisuals: usize,
    SetUseSystemFocusVisuals: usize,
    XYFocusLeft: usize,
    SetXYFocusLeft: usize,
    XYFocusRight: usize,
    SetXYFocusRight: usize,
    XYFocusUp: usize,
    SetXYFocusUp: usize,
    XYFocusDown: usize,
    SetXYFocusDown: usize,
    IsTabStop: usize,
    SetIsTabStop: usize,
    TabIndex: usize,
    SetTabIndex: usize,
    KeyUp: usize,
    RemoveKeyUp: usize,
    pub KeyDown: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveKeyDown:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub GotFocus: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveGotFocus:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub LostFocus: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveLostFocus:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    DragStarting: usize,
    RemoveDragStarting: usize,
    DropCompleted: usize,
    RemoveDropCompleted: usize,
    CharacterReceived: usize,
    RemoveCharacterReceived: usize,
    DragEnter: usize,
    RemoveDragEnter: usize,
    DragLeave: usize,
    RemoveDragLeave: usize,
    DragOver: usize,
    RemoveDragOver: usize,
    Drop: usize,
    RemoveDrop: usize,
    pub PointerPressed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerPressed:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerMoved: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerMoved:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerReleased: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerReleased:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerEntered: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerEntered:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerExited: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerExited:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    pub PointerCaptureLost: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePointerCaptureLost:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    PointerCanceled: usize,
    RemovePointerCanceled: usize,
    PointerWheelChanged: usize,
    RemovePointerWheelChanged: usize,
    Tapped: usize,
    RemoveTapped: usize,
    DoubleTapped: usize,
    RemoveDoubleTapped: usize,
    Holding: usize,
    RemoveHolding: usize,
    ContextRequested: usize,
    RemoveContextRequested: usize,
    ContextCanceled: usize,
    RemoveContextCanceled: usize,
    RightTapped: usize,
    RemoveRightTapped: usize,
    ManipulationStarting: usize,
    RemoveManipulationStarting: usize,
    ManipulationInertiaStarting: usize,
    RemoveManipulationInertiaStarting: usize,
    ManipulationStarted: usize,
    RemoveManipulationStarted: usize,
    ManipulationDelta: usize,
    RemoveManipulationDelta: usize,
    ManipulationCompleted: usize,
    RemoveManipulationCompleted: usize,
    AccessKeyDisplayRequested: usize,
    RemoveAccessKeyDisplayRequested: usize,
    AccessKeyDisplayDismissed: usize,
    RemoveAccessKeyDisplayDismissed: usize,
    AccessKeyInvoked: usize,
    RemoveAccessKeyInvoked: usize,
    ProcessKeyboardAccelerators: usize,
    RemoveProcessKeyboardAccelerators: usize,
    GettingFocus: usize,
    RemoveGettingFocus: usize,
    LosingFocus: usize,
    RemoveLosingFocus: usize,
    NoFocusCandidateFound: usize,
    RemoveNoFocusCandidateFound: usize,
    pub PreviewKeyDown: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemovePreviewKeyDown:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    PreviewKeyUp: usize,
    RemovePreviewKeyUp: usize,
    BringIntoViewRequested: usize,
    RemoveBringIntoViewRequested: usize,
    pub Measure: unsafe extern "system" fn(*mut core::ffi::c_void, Size) -> windows_core::HRESULT,
    Arrange: usize,
    pub CapturePointer: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut bool,
    ) -> windows_core::HRESULT,
    pub ReleasePointerCapture: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ReleasePointerCaptures: usize,
    AddHandler: usize,
    RemoveHandler: usize,
    pub TransformToVisual: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    InvalidateMeasure: usize,
    InvalidateArrange: usize,
    pub UpdateLayout: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    CancelDirectManipulations: usize,
    StartDragAsync: usize,
    StartBringIntoView: usize,
    StartBringIntoViewWithOptions: usize,
    TryInvokeKeyboardAccelerator: usize,
    pub Focus: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        FocusState,
        *mut bool,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUIElementProtected,
    IUIElementProtected_Vtbl,
    0x8f69b9e9_1f00_5834_9bf1_a9257bed39f0
);
impl windows_core::RuntimeType for IUIElementProtected {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IUIElementProtected {
    pub fn SetProtectedCursor<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<InputCursor>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetProtectedCursor)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IUIElementProtected_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    ProtectedCursor: usize,
    pub SetProtectedCursor: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IUriRuntimeClass,
    IUriRuntimeClass_Vtbl,
    0x9e365e57_48b2_4160_956f_c7385120bbfc
);
impl windows_core::RuntimeType for IUriRuntimeClass {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IUriRuntimeClass_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IUriRuntimeClassFactory,
    IUriRuntimeClassFactory_Vtbl,
    0x44a9796f_723e_4fdf_a218_033e75b0c084
);
impl windows_core::RuntimeType for IUriRuntimeClassFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IUriRuntimeClassFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateUri: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IVisualTreeHelper,
    IVisualTreeHelper_Vtbl,
    0x5f69ac1e_6504_5e3f_a11c_87684c1db814
);
impl windows_core::RuntimeType for IVisualTreeHelper {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IVisualTreeHelper_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IVisualTreeHelperStatics,
    IVisualTreeHelperStatics_Vtbl,
    0x5aece43c_7651_5bb5_855c_2198496e455e
);
impl windows_core::RuntimeType for IVisualTreeHelperStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IVisualTreeHelperStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    FindElementsInHostCoordinatesPoint: usize,
    FindElementsInHostCoordinatesRect: usize,
    FindAllElementsInHostCoordinatesPoint: usize,
    FindAllElementsInHostCoordinatesRect: usize,
    pub GetChild: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        i32,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetChildrenCount: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i32,
    ) -> windows_core::HRESULT,
    pub GetParent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWebView2,
    IWebView2_Vtbl,
    0x2b2c76c2_997c_5069_a8f0_9b84cd7e624b
);
impl windows_core::RuntimeType for IWebView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWebView2 {
    pub fn CoreWebView2(&self) -> windows_core::Result<CoreWebView2> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).CoreWebView2)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn Close(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Close)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
}
#[repr(C)]
pub struct IWebView2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CoreWebView2: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    EnsureCoreWebView2Async: usize,
    ExecuteScriptAsync: usize,
    Source: usize,
    SetSource: usize,
    CanGoForward: usize,
    SetCanGoForward: usize,
    CanGoBack: usize,
    SetCanGoBack: usize,
    DefaultBackgroundColor: usize,
    SetDefaultBackgroundColor: usize,
    Reload: usize,
    GoForward: usize,
    GoBack: usize,
    NavigateToString: usize,
    pub Close: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWebView22,
    IWebView22_Vtbl,
    0x560c5eed_3e7a_51e4_b14f_107ba02b89be
);
impl windows_core::RuntimeType for IWebView22 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWebView22 {
    pub fn EnsureCoreWebView2WithEnvironmentAsync<P0>(
        &self,
        environment: P0,
    ) -> windows_core::Result<windows_future::IAsyncAction>
    where
        P0: windows_core::Param<CoreWebView2Environment>,
    {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).EnsureCoreWebView2WithEnvironmentAsync)(
                windows_core::Interface::as_raw(self),
                environment.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IWebView22_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub EnsureCoreWebView2WithEnvironmentAsync: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    )
        -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWebView2Factory,
    IWebView2Factory_Vtbl,
    0xfb4ec2ce_3074_5c42_b655_64fb81fbd040
);
impl windows_core::RuntimeType for IWebView2Factory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IWebView2Factory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindow,
    IWindow_Vtbl,
    0x61f0ec79_5d52_56b5_86fb_40fa4af288b0
);
impl windows_core::RuntimeType for IWindow {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWindow {
    pub fn SetContent<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetContent)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn SetTitle(&self, value: &str) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetTitle)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(value)),
            )
            .ok()
        }
    }
    pub fn SetExtendsContentIntoTitleBar(&self, value: bool) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).SetExtendsContentIntoTitleBar)(
                windows_core::Interface::as_raw(self),
                value,
            )
            .ok()
        }
    }
    pub fn Closed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<WindowEventArgs>)
            + 'static,
    {
        let handler: TypedEventHandler<windows_core::IInspectable, WindowEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<windows_core::IInspectable, WindowEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<windows_core::IInspectable, WindowEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Closed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveClosed,
            ))
        }
    }
    pub fn Activate(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Activate)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn Close(&self) -> windows_core::Result<()> {
        unsafe {
            (windows_core::Interface::vtable(self).Close)(windows_core::Interface::as_raw(self))
                .ok()
        }
    }
    pub fn SetTitleBar<P0>(&self, titlebar: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<UIElement>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetTitleBar)(
                windows_core::Interface::as_raw(self),
                titlebar.param().abi(),
            )
            .ok()
        }
    }
}
#[repr(C)]
pub struct IWindow_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Bounds: usize,
    Visible: usize,
    Content: usize,
    pub SetContent: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    CoreWindow: usize,
    Compositor: usize,
    Dispatcher: usize,
    DispatcherQueue: usize,
    Title: usize,
    pub SetTitle: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    ExtendsContentIntoTitleBar: usize,
    pub SetExtendsContentIntoTitleBar:
        unsafe extern "system" fn(*mut core::ffi::c_void, bool) -> windows_core::HRESULT,
    Activated: usize,
    RemoveActivated: usize,
    pub Closed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveClosed:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
    SizeChanged: usize,
    RemoveSizeChanged: usize,
    VisibilityChanged: usize,
    RemoveVisibilityChanged: usize,
    pub Activate: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub Close: unsafe extern "system" fn(*mut core::ffi::c_void) -> windows_core::HRESULT,
    pub SetTitleBar: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindow2,
    IWindow2_Vtbl,
    0x42febaa5_1c32_522a_a591_57618c6f665d
);
impl windows_core::RuntimeType for IWindow2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IWindow2 {
    pub fn SetSystemBackdrop<P0>(&self, value: P0) -> windows_core::Result<()>
    where
        P0: windows_core::Param<SystemBackdrop>,
    {
        unsafe {
            (windows_core::Interface::vtable(self).SetSystemBackdrop)(
                windows_core::Interface::as_raw(self),
                value.param().abi(),
            )
            .ok()
        }
    }
    pub fn AppWindow(&self) -> windows_core::Result<AppWindow> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).AppWindow)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
}
#[repr(C)]
pub struct IWindow2_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    SystemBackdrop: usize,
    pub SetSystemBackdrop: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub AppWindow: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IWindowEventArgs,
    IWindowEventArgs_Vtbl,
    0x1140827c_fe0a_5268_bc2b_f4492c2ccb49
);
impl windows_core::RuntimeType for IWindowEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IWindowEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IWindowFactory,
    IWindowFactory_Vtbl,
    0xf0441536_afef_5222_918f_324a9b2dec75
);
impl windows_core::RuntimeType for IWindowFactory {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IWindowFactory_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub CreateInstance: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlControlsResources,
    IXamlControlsResources_Vtbl,
    0x918ca043_f42c_5805_861b_62d6d1d0c162
);
impl windows_core::RuntimeType for IXamlControlsResources {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IXamlControlsResources_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlMetadataProvider,
    IXamlMetadataProvider_Vtbl,
    0xa96251f0_2214_5d53_8746_ce99a2593cd7
);
impl windows_core::RuntimeType for IXamlMetadataProvider {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
    const NAME: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"Microsoft.UI.Xaml.Markup.IXamlMetadataProvider",
    );
}
windows_core::imp::interface_hierarchy!(
    IXamlMetadataProvider,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl IXamlMetadataProvider {
    pub fn GetXamlType(&self, r#type: &TypeName) -> windows_core::Result<IXamlType> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetXamlType)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(r#type),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetXamlTypeByFullName(&self, fullname: &str) -> windows_core::Result<IXamlType> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).GetXamlTypeByFullName)(
                windows_core::Interface::as_raw(self),
                core::mem::transmute_copy(&windows_core::HSTRING::from(fullname)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        }
    }
    pub fn GetXmlnsDefinitions(
        &self,
    ) -> windows_core::Result<windows_core::Array<XmlnsDefinition>> {
        unsafe {
            let mut result__ = core::mem::MaybeUninit::zeroed();
            (windows_core::Interface::vtable(self).GetXmlnsDefinitions)(
                windows_core::Interface::as_raw(self),
                windows_core::Array::<XmlnsDefinition>::set_abi_len(core::mem::transmute(
                    &mut result__,
                )),
                result__.as_mut_ptr() as *mut _ as _,
            )
            .map(|| result__.assume_init())
        }
    }
}
impl windows_core::RuntimeName for IXamlMetadataProvider {
    const NAME: &'static str = "Microsoft.UI.Xaml.Markup.IXamlMetadataProvider";
}
pub trait IXamlMetadataProvider_Impl: windows_core::IUnknownImpl {
    fn GetXamlType(&self, r#type: &TypeName) -> windows_core::Result<IXamlType>;
    fn GetXamlTypeByFullName(
        &self,
        fullName: &windows_core::HSTRING,
    ) -> windows_core::Result<IXamlType>;
    fn GetXmlnsDefinitions(&self) -> windows_core::Result<windows_core::Array<XmlnsDefinition>>;
}
impl IXamlMetadataProvider_Vtbl {
    pub const fn new<Identity: IXamlMetadataProvider_Impl, const OFFSET: isize>() -> Self {
        unsafe extern "system" fn GetXamlType<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            r#type: core::mem::MaybeUninit<TypeName>,
            result__: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXamlType(this, core::mem::transmute(&r#type)) {
                    Ok(ok__) => {
                        result__.write(core::mem::transmute_copy(&ok__));
                        core::mem::forget(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn GetXamlTypeByFullName<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            fullname: *mut core::ffi::c_void,
            result__: *mut *mut core::ffi::c_void,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXamlTypeByFullName(
                    this,
                    core::mem::transmute(&fullname),
                ) {
                    Ok(ok__) => {
                        result__.write(core::mem::transmute_copy(&ok__));
                        core::mem::forget(ok__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        unsafe extern "system" fn GetXmlnsDefinitions<
            Identity: IXamlMetadataProvider_Impl,
            const OFFSET: isize,
        >(
            this: *mut core::ffi::c_void,
            result_size__: *mut u32,
            result__: *mut *mut core::mem::MaybeUninit<XmlnsDefinition>,
        ) -> windows_core::HRESULT {
            unsafe {
                let this: &Identity =
                    &*((this as *const *const ()).offset(OFFSET) as *const Identity);
                match IXamlMetadataProvider_Impl::GetXmlnsDefinitions(this) {
                    Ok(ok__) => {
                        let (ok_data__, ok_data_len__) = ok__.into_abi();
                        result__.write(ok_data__);
                        result_size__.write(ok_data_len__);
                        windows_core::HRESULT(0)
                    }
                    Err(err) => err.into(),
                }
            }
        }
        Self {
            base__: windows_core::IInspectable_Vtbl::new::<Identity, IXamlMetadataProvider, OFFSET>(
            ),
            GetXamlType: GetXamlType::<Identity, OFFSET>,
            GetXamlTypeByFullName: GetXamlTypeByFullName::<Identity, OFFSET>,
            GetXmlnsDefinitions: GetXmlnsDefinitions::<Identity, OFFSET>,
        }
    }
    pub fn matches(iid: &windows_core::GUID) -> bool {
        iid == &<IXamlMetadataProvider as windows_core::Interface>::IID
    }
}
#[repr(C)]
pub struct IXamlMetadataProvider_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub GetXamlType: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        core::mem::MaybeUninit<TypeName>,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetXamlTypeByFullName: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
    pub GetXmlnsDefinitions: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut u32,
        *mut *mut core::mem::MaybeUninit<XmlnsDefinition>,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlReader,
    IXamlReader_Vtbl,
    0x54ce54c8_38c6_50d9_ac98_4b03eddbde9f
);
impl windows_core::RuntimeType for IXamlReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IXamlReader_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlReaderStatics,
    IXamlReaderStatics_Vtbl,
    0x82a4cd9e_435e_5aeb_8c4f_300cece45cae
);
impl windows_core::RuntimeType for IXamlReaderStatics {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IXamlReaderStatics_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    pub Load: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlRoot,
    IXamlRoot_Vtbl,
    0x60cb215a_ad15_520a_8b01_4416824f0441
);
impl windows_core::RuntimeType for IXamlRoot {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
impl IXamlRoot {
    pub fn RasterizationScale(&self) -> windows_core::Result<f64> {
        unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(self).RasterizationScale)(
                windows_core::Interface::as_raw(self),
                &mut result__,
            )
            .map(|| result__)
        }
    }
    pub fn Changed<F>(&self, handler: F) -> windows_core::Result<windows_core::EventRevoker>
    where
        F: Fn(windows_core::Ref<XamlRoot>, windows_core::Ref<XamlRootChangedEventArgs>) + 'static,
    {
        let handler: TypedEventHandler<XamlRoot, XamlRootChangedEventArgs> = {
            let com = windows_core::imp::DelegateBox::<
                TypedEventHandler<XamlRoot, XamlRootChangedEventArgs>,
                F,
            >::new(
                &TypedEventHandlerBox::<XamlRoot, XamlRootChangedEventArgs, F>::VTABLE,
                handler,
            );
            unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
        };
        unsafe {
            let mut result__ = core::mem::zeroed();
            let token__ = (windows_core::Interface::vtable(self).Changed)(
                windows_core::Interface::as_raw(self),
                windows_core::Interface::as_raw(&handler),
                &mut result__,
            )
            .map(|| result__)?;
            Ok(windows_core::EventRevoker::new(
                self.clone(),
                token__,
                windows_core::Interface::vtable(self).RemoveChanged,
            ))
        }
    }
}
#[repr(C)]
pub struct IXamlRoot_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
    Content: usize,
    Size: usize,
    pub RasterizationScale:
        unsafe extern "system" fn(*mut core::ffi::c_void, *mut f64) -> windows_core::HRESULT,
    IsHostVisible: usize,
    pub Changed: unsafe extern "system" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        *mut i64,
    ) -> windows_core::HRESULT,
    pub RemoveChanged:
        unsafe extern "system" fn(*mut core::ffi::c_void, i64) -> windows_core::HRESULT,
}
windows_core::imp::define_interface!(
    IXamlRootChangedEventArgs,
    IXamlRootChangedEventArgs_Vtbl,
    0x61d2c719_f8a1_515a_902c_cfa498ba7a7f
);
impl windows_core::RuntimeType for IXamlRootChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct IXamlRootChangedEventArgs_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
windows_core::imp::define_interface!(
    IXamlType,
    IXamlType_Vtbl,
    0xd24219df_7ec9_57f1_a27b_6af251d9c5bc
);
impl windows_core::RuntimeType for IXamlType {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
windows_core::imp::interface_hierarchy!(
    IXamlType,
    windows_core::IUnknown,
    windows_core::IInspectable
);
#[repr(C)]
pub struct IXamlType_Vtbl {
    pub base__: windows_core::IInspectable_Vtbl,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IconElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    IconElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(IconElement, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for IconElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IIconElement>();
}
unsafe impl windows_core::Interface for IconElement {
    type Vtable = <IIconElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IIconElement as windows_core::Interface>::IID;
}
impl core::ops::Deref for IconElement {
    type Target = IIconElement;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for IconElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.IconElement";
}
unsafe impl Send for IconElement {}
unsafe impl Sync for IconElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Image(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Image, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Image, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Image {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IImage>();
}
unsafe impl windows_core::Interface for Image {
    type Vtable = <IImage as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IImage as windows_core::Interface>::IID;
}
impl core::ops::Deref for Image {
    type Target = IImage;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Image {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Image";
}
unsafe impl Send for Image {}
unsafe impl Sync for Image {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageIcon(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ImageIcon,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ImageIcon,
    IconElement,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ImageIcon {
    pub fn new() -> windows_core::Result<Self> {
        Self::IImageIconFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IImageIconFactory<R, F: FnOnce(&IImageIconFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ImageIcon, IImageIconFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ImageIcon {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IImageIcon>();
}
unsafe impl windows_core::Interface for ImageIcon {
    type Vtable = <IImageIcon as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IImageIcon as windows_core::Interface>::IID;
}
impl core::ops::Deref for ImageIcon {
    type Target = IImageIcon;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ImageIcon {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ImageIcon";
}
unsafe impl Send for ImageIcon {}
unsafe impl Sync for ImageIcon {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageSource(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ImageSource,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ImageSource, DependencyObject);
impl windows_core::RuntimeType for ImageSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IImageSource>();
}
unsafe impl windows_core::Interface for ImageSource {
    type Vtable = <IImageSource as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IImageSource as windows_core::Interface>::IID;
}
impl core::ops::Deref for ImageSource {
    type Target = IImageSource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ImageSource {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.ImageSource";
}
unsafe impl Send for ImageSource {}
unsafe impl Sync for ImageSource {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InMemoryRandomAccessStream(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InMemoryRandomAccessStream,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IRandomAccessStream
);
impl InMemoryRandomAccessStream {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InMemoryRandomAccessStream,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InMemoryRandomAccessStream {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRandomAccessStream>();
}
unsafe impl windows_core::Interface for InMemoryRandomAccessStream {
    type Vtable = <IRandomAccessStream as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRandomAccessStream as windows_core::Interface>::IID;
}
impl core::ops::Deref for InMemoryRandomAccessStream {
    type Target = IRandomAccessStream;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for InMemoryRandomAccessStream {
    const NAME: &'static str = "Windows.Storage.Streams.InMemoryRandomAccessStream";
}
unsafe impl Send for InMemoryRandomAccessStream {}
unsafe impl Sync for InMemoryRandomAccessStream {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InfoBar(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InfoBar,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    InfoBar,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for InfoBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInfoBar>();
}
unsafe impl windows_core::Interface for InfoBar {
    type Vtable = <IInfoBar as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInfoBar as windows_core::Interface>::IID;
}
impl core::ops::Deref for InfoBar {
    type Target = IInfoBar;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for InfoBar {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.InfoBar";
}
unsafe impl Send for InfoBar {}
unsafe impl Sync for InfoBar {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InfoBarSeverity(pub i32);
impl InfoBarSeverity {
    pub const Informational: Self = Self(0);
    pub const Success: Self = Self(1);
    pub const Warning: Self = Self(2);
    pub const Error: Self = Self(3);
}
impl windows_core::imp::TypeKind for InfoBarSeverity {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for InfoBarSeverity {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Controls.InfoBarSeverity;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputCursor(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputCursor,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(InputCursor, IClosable);
impl windows_core::RuntimeType for InputCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputCursor>();
}
unsafe impl windows_core::Interface for InputCursor {
    type Vtable = <IInputCursor as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputCursor as windows_core::Interface>::IID;
}
impl core::ops::Deref for InputCursor {
    type Target = IInputCursor;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for InputCursor {
    const NAME: &'static str = "Microsoft.UI.Input.InputCursor";
}
unsafe impl Send for InputCursor {}
unsafe impl Sync for InputCursor {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputNonClientPointerSource(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputNonClientPointerSource,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl InputNonClientPointerSource {
    pub fn GetForWindowId(windowid: WindowId) -> windows_core::Result<Self> {
        Self::IInputNonClientPointerSourceStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetForWindowId)(
                windows_core::Interface::as_raw(this),
                windowid,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IInputNonClientPointerSourceStatics<
        R,
        F: FnOnce(&IInputNonClientPointerSourceStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputNonClientPointerSource,
            IInputNonClientPointerSourceStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InputNonClientPointerSource {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputNonClientPointerSource>();
}
unsafe impl windows_core::Interface for InputNonClientPointerSource {
    type Vtable = <IInputNonClientPointerSource as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputNonClientPointerSource as windows_core::Interface>::IID;
}
impl core::ops::Deref for InputNonClientPointerSource {
    type Target = IInputNonClientPointerSource;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for InputNonClientPointerSource {
    const NAME: &'static str = "Microsoft.UI.Input.InputNonClientPointerSource";
}
unsafe impl Send for InputNonClientPointerSource {}
unsafe impl Sync for InputNonClientPointerSource {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputSystemCursor(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    InputSystemCursor,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(InputSystemCursor, InputCursor);
impl InputSystemCursor {
    pub fn Create(r#type: InputSystemCursorShape) -> windows_core::Result<Self> {
        Self::IInputSystemCursorStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Create)(
                windows_core::Interface::as_raw(this),
                r#type,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IInputSystemCursorStatics<
        R,
        F: FnOnce(&IInputSystemCursorStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            InputSystemCursor,
            IInputSystemCursorStatics,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for InputSystemCursor {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IInputSystemCursor>();
}
unsafe impl windows_core::Interface for InputSystemCursor {
    type Vtable = <IInputSystemCursor as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IInputSystemCursor as windows_core::Interface>::IID;
}
impl core::ops::Deref for InputSystemCursor {
    type Target = IInputSystemCursor;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for InputSystemCursor {
    const NAME: &'static str = "Microsoft.UI.Input.InputSystemCursor";
}
unsafe impl Send for InputSystemCursor {}
unsafe impl Sync for InputSystemCursor {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InputSystemCursorShape(pub i32);
impl InputSystemCursorShape {
    pub const Arrow: Self = Self(0);
    pub const Cross: Self = Self(1);
    pub const Hand: Self = Self(3);
    pub const Help: Self = Self(4);
    pub const IBeam: Self = Self(5);
    pub const SizeAll: Self = Self(6);
    pub const SizeNortheastSouthwest: Self = Self(7);
    pub const SizeNorthSouth: Self = Self(8);
    pub const SizeNorthwestSoutheast: Self = Self(9);
    pub const SizeWestEast: Self = Self(10);
    pub const UniversalNo: Self = Self(11);
    pub const UpArrow: Self = Self(12);
    pub const Wait: Self = Self(13);
    pub const Pin: Self = Self(14);
    pub const Person: Self = Self(15);
    pub const AppStarting: Self = Self(16);
}
impl windows_core::imp::TypeKind for InputSystemCursorShape {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for InputSystemCursorShape {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Input.InputSystemCursorShape;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemClickEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemClickEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ItemClickEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for ItemClickEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IItemClickEventArgs>();
}
unsafe impl windows_core::Interface for ItemClickEventArgs {
    type Vtable = <IItemClickEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IItemClickEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for ItemClickEventArgs {
    type Target = IItemClickEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ItemClickEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemClickEventArgs";
}
unsafe impl Send for ItemClickEventArgs {}
unsafe impl Sync for ItemClickEventArgs {}
windows_core::imp::define_interface!(
    ItemClickEventHandler,
    ItemClickEventHandler_Vtbl,
    0xa3903624_3393_566c_a6b9_a6b4b3e301c3
);
impl windows_core::RuntimeType for ItemClickEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct ItemClickEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct ItemClickEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<ItemClickEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<ItemClickEventArgs>)
        + 'static,
> ItemClickEventHandlerBox<F>
{
    const VTABLE: ItemClickEventHandler_Vtbl = ItemClickEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<ItemClickEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<ItemClickEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<ItemClickEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<ItemClickEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IObservableVector<windows_core::IInspectable>
);
impl windows_core::RuntimeType for ItemCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::for_class::<
        Self,
        windows_collections::IObservableVector<windows_core::IInspectable>,
    >();
}
unsafe impl windows_core::Interface for ItemCollection {
    type Vtable = < windows_collections::IObservableVector < windows_core::IInspectable > as windows_core::Interface >::Vtable ;
    const IID: windows_core::GUID = <windows_collections::IObservableVector<
        windows_core::IInspectable,
    > as windows_core::Interface>::IID;
}
impl core::ops::Deref for ItemCollection {
    type Target = windows_collections::IObservableVector<windows_core::IInspectable>;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ItemCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemCollection";
}
unsafe impl Send for ItemCollection {}
unsafe impl Sync for ItemCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemsControl(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ItemsControl,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ItemsControl {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IItemsControl>();
}
unsafe impl windows_core::Interface for ItemsControl {
    type Vtable = <IItemsControl as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IItemsControl as windows_core::Interface>::IID;
}
impl core::ops::Deref for ItemsControl {
    type Target = IItemsControl;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ItemsControl {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ItemsControl";
}
unsafe impl Send for ItemsControl {}
unsafe impl Sync for ItemsControl {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KEYBDINPUT {
    pub wVk: u16,
    pub wScan: u16,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
pub const KEYEVENTF_KEYUP: i32 = 2;
pub const KF_FLAG_DEFAULT: KNOWN_FOLDER_FLAG = 0;
pub type KNOWNFOLDERID = windows_core::GUID;
pub type KNOWN_FOLDER_FLAG = u32;
windows_core::imp::define_interface!(
    KeyEventHandler,
    KeyEventHandler_Vtbl,
    0xdb68e7cc_9a2b_527d_9989_25284daccc03
);
impl windows_core::RuntimeType for KeyEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct KeyEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct KeyEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<KeyRoutedEventArgs>)
        + 'static,
> KeyEventHandlerBox<F>
{
    const VTABLE: KeyEventHandler_Vtbl = KeyEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<KeyEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<KeyEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<KeyEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<KeyEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    KeyRoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(KeyRoutedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for KeyRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IKeyRoutedEventArgs>();
}
unsafe impl windows_core::Interface for KeyRoutedEventArgs {
    type Vtable = <IKeyRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IKeyRoutedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for KeyRoutedEventArgs {
    type Target = IKeyRoutedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for KeyRoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.KeyRoutedEventArgs";
}
unsafe impl Send for KeyRoutedEventArgs {}
unsafe impl Sync for KeyRoutedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardAccelerator(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    KeyboardAccelerator,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(KeyboardAccelerator, DependencyObject);
impl KeyboardAccelerator {
    pub fn new() -> windows_core::Result<Self> {
        Self::IKeyboardAcceleratorFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IKeyboardAcceleratorFactory<
        R,
        F: FnOnce(&IKeyboardAcceleratorFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            KeyboardAccelerator,
            IKeyboardAcceleratorFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for KeyboardAccelerator {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IKeyboardAccelerator>();
}
unsafe impl windows_core::Interface for KeyboardAccelerator {
    type Vtable = <IKeyboardAccelerator as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IKeyboardAccelerator as windows_core::Interface>::IID;
}
impl core::ops::Deref for KeyboardAccelerator {
    type Target = IKeyboardAccelerator;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for KeyboardAccelerator {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.KeyboardAccelerator";
}
unsafe impl Send for KeyboardAccelerator {}
unsafe impl Sync for KeyboardAccelerator {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardAcceleratorInvokedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    KeyboardAcceleratorInvokedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for KeyboardAcceleratorInvokedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IKeyboardAcceleratorInvokedEventArgs>();
}
unsafe impl windows_core::Interface for KeyboardAcceleratorInvokedEventArgs {
    type Vtable = <IKeyboardAcceleratorInvokedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <IKeyboardAcceleratorInvokedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for KeyboardAcceleratorInvokedEventArgs {
    type Target = IKeyboardAcceleratorInvokedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for KeyboardAcceleratorInvokedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.KeyboardAcceleratorInvokedEventArgs";
}
unsafe impl Send for KeyboardAcceleratorInvokedEventArgs {}
unsafe impl Sync for KeyboardAcceleratorInvokedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyboardAcceleratorPlacementMode(pub i32);
impl KeyboardAcceleratorPlacementMode {
    pub const Auto: Self = Self(0);
    pub const Hidden: Self = Self(1);
}
impl windows_core::imp::TypeKind for KeyboardAcceleratorPlacementMode {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for KeyboardAcceleratorPlacementMode {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Input.KeyboardAcceleratorPlacementMode;i4)",
    );
}
pub const LOAD_LIBRARY_SEARCH_APPLICATION_DIR: i32 = 512;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub const LR_DEFAULTSIZE: i32 = 64;
pub const LR_SHARED: i32 = 32768;
pub type LSTATUS = i32;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchActivatedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    LaunchActivatedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for LaunchActivatedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ILaunchActivatedEventArgs>();
}
unsafe impl windows_core::Interface for LaunchActivatedEventArgs {
    type Vtable = <ILaunchActivatedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ILaunchActivatedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for LaunchActivatedEventArgs {
    type Target = ILaunchActivatedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for LaunchActivatedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.LaunchActivatedEventArgs";
}
unsafe impl Send for LaunchActivatedEventArgs {}
unsafe impl Sync for LaunchActivatedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ListView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ListView,
    ListViewBase,
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ListView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IListView>();
}
unsafe impl windows_core::Interface for ListView {
    type Vtable = <IListView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IListView as windows_core::Interface>::IID;
}
impl core::ops::Deref for ListView {
    type Target = IListView;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ListView {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ListView";
}
unsafe impl Send for ListView {}
unsafe impl Sync for ListView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListViewBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ListViewBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ListViewBase,
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ListViewBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IListViewBase>();
}
unsafe impl windows_core::Interface for ListViewBase {
    type Vtable = <IListViewBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IListViewBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for ListViewBase {
    type Target = IListViewBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ListViewBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ListViewBase";
}
unsafe impl Send for ListViewBase {}
unsafe impl Sync for ListViewBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ListViewItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ListViewItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ListViewItem,
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ListViewItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::IListViewItemFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IListViewItemFactory<R, F: FnOnce(&IListViewItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ListViewItem, IListViewItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ListViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IListViewItem>();
}
unsafe impl windows_core::Interface for ListViewItem {
    type Vtable = <IListViewItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IListViewItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for ListViewItem {
    type Target = IListViewItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ListViewItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ListViewItem";
}
unsafe impl Send for ListViewItem {}
unsafe impl Sync for ListViewItem {}
pub const MB_ICONERROR: i32 = 16;
pub const MB_OK: i32 = 0;
pub const MB_YESNO: i32 = 4;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MOUSEINPUT {
    pub dx: i32,
    pub dy: i32,
    pub mouseData: u32,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFlyout(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MenuFlyout,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(MenuFlyout, FlyoutBase, DependencyObject);
impl MenuFlyout {
    pub fn new() -> windows_core::Result<Self> {
        Self::IMenuFlyoutFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IMenuFlyoutFactory<R, F: FnOnce(&IMenuFlyoutFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<MenuFlyout, IMenuFlyoutFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for MenuFlyout {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMenuFlyout>();
}
unsafe impl windows_core::Interface for MenuFlyout {
    type Vtable = <IMenuFlyout as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMenuFlyout as windows_core::Interface>::IID;
}
impl core::ops::Deref for MenuFlyout {
    type Target = IMenuFlyout;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MenuFlyout {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.MenuFlyout";
}
unsafe impl Send for MenuFlyout {}
unsafe impl Sync for MenuFlyout {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFlyoutItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MenuFlyoutItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    MenuFlyoutItem,
    MenuFlyoutItemBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl MenuFlyoutItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::IMenuFlyoutItemFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IMenuFlyoutItemFactory<R, F: FnOnce(&IMenuFlyoutItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<MenuFlyoutItem, IMenuFlyoutItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for MenuFlyoutItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMenuFlyoutItem>();
}
unsafe impl windows_core::Interface for MenuFlyoutItem {
    type Vtable = <IMenuFlyoutItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMenuFlyoutItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for MenuFlyoutItem {
    type Target = IMenuFlyoutItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MenuFlyoutItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.MenuFlyoutItem";
}
unsafe impl Send for MenuFlyoutItem {}
unsafe impl Sync for MenuFlyoutItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFlyoutItemBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MenuFlyoutItemBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    MenuFlyoutItemBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for MenuFlyoutItemBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMenuFlyoutItemBase>();
}
unsafe impl windows_core::Interface for MenuFlyoutItemBase {
    type Vtable = <IMenuFlyoutItemBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMenuFlyoutItemBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for MenuFlyoutItemBase {
    type Target = IMenuFlyoutItemBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MenuFlyoutItemBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.MenuFlyoutItemBase";
}
unsafe impl Send for MenuFlyoutItemBase {}
unsafe impl Sync for MenuFlyoutItemBase {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFlyoutSeparator(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MenuFlyoutSeparator,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    MenuFlyoutSeparator,
    MenuFlyoutItemBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl MenuFlyoutSeparator {
    pub fn new() -> windows_core::Result<Self> {
        Self::IMenuFlyoutSeparatorFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IMenuFlyoutSeparatorFactory<
        R,
        F: FnOnce(&IMenuFlyoutSeparatorFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            MenuFlyoutSeparator,
            IMenuFlyoutSeparatorFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for MenuFlyoutSeparator {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMenuFlyoutSeparator>();
}
unsafe impl windows_core::Interface for MenuFlyoutSeparator {
    type Vtable = <IMenuFlyoutSeparator as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMenuFlyoutSeparator as windows_core::Interface>::IID;
}
impl core::ops::Deref for MenuFlyoutSeparator {
    type Target = IMenuFlyoutSeparator;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MenuFlyoutSeparator {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.MenuFlyoutSeparator";
}
unsafe impl Send for MenuFlyoutSeparator {}
unsafe impl Sync for MenuFlyoutSeparator {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MenuFlyoutSubItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MenuFlyoutSubItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    MenuFlyoutSubItem,
    MenuFlyoutItemBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl MenuFlyoutSubItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            MenuFlyoutSubItem,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for MenuFlyoutSubItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMenuFlyoutSubItem>();
}
unsafe impl windows_core::Interface for MenuFlyoutSubItem {
    type Vtable = <IMenuFlyoutSubItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMenuFlyoutSubItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for MenuFlyoutSubItem {
    type Target = IMenuFlyoutSubItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MenuFlyoutSubItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.MenuFlyoutSubItem";
}
unsafe impl Send for MenuFlyoutSubItem {}
unsafe impl Sync for MenuFlyoutSubItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MicaBackdrop(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    MicaBackdrop,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(MicaBackdrop, SystemBackdrop, DependencyObject);
impl MicaBackdrop {
    pub fn new() -> windows_core::Result<Self> {
        Self::IMicaBackdropFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IMicaBackdropFactory<R, F: FnOnce(&IMicaBackdropFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<MicaBackdrop, IMicaBackdropFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for MicaBackdrop {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IMicaBackdrop>();
}
unsafe impl windows_core::Interface for MicaBackdrop {
    type Vtable = <IMicaBackdrop as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IMicaBackdrop as windows_core::Interface>::IID;
}
impl core::ops::Deref for MicaBackdrop {
    type Target = IMicaBackdrop;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for MicaBackdrop {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.MicaBackdrop";
}
unsafe impl Send for MicaBackdrop {}
unsafe impl Sync for MicaBackdrop {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NonClientRegionKind(pub i32);
impl NonClientRegionKind {
    pub const Close: Self = Self(0);
    pub const Maximize: Self = Self(1);
    pub const Minimize: Self = Self(2);
    pub const Icon: Self = Self(3);
    pub const Caption: Self = Self(4);
    pub const TopBorder: Self = Self(5);
    pub const LeftBorder: Self = Self(6);
    pub const BottomBorder: Self = Self(7);
    pub const RightBorder: Self = Self(8);
    pub const Passthrough: Self = Self(9);
}
impl windows_core::imp::TypeKind for NonClientRegionKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for NonClientRegionKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Input.NonClientRegionKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverlappedPresenter(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    OverlappedPresenter,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(OverlappedPresenter, AppWindowPresenter);
impl windows_core::RuntimeType for OverlappedPresenter {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IOverlappedPresenter>();
}
unsafe impl windows_core::Interface for OverlappedPresenter {
    type Vtable = <IOverlappedPresenter as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IOverlappedPresenter as windows_core::Interface>::IID;
}
impl core::ops::Deref for OverlappedPresenter {
    type Target = IOverlappedPresenter;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for OverlappedPresenter {
    const NAME: &'static str = "Microsoft.UI.Windowing.OverlappedPresenter";
}
unsafe impl Send for OverlappedPresenter {}
unsafe impl Sync for OverlappedPresenter {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OverlappedPresenterState(pub i32);
impl OverlappedPresenterState {
    pub const Maximized: Self = Self(0);
    pub const Minimized: Self = Self(1);
    pub const Restored: Self = Self(2);
}
impl windows_core::imp::TypeKind for OverlappedPresenterState {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for OverlappedPresenterState {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Windowing.OverlappedPresenterState;i4)",
    );
}
pub type PACKAGEDEPENDENCY_CONTEXT = *mut core::ffi::c_void;
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PACKAGE_VERSION {
    pub Anonymous: PACKAGE_VERSION_0,
}
impl Default for PACKAGE_VERSION {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C, packed(4))]
#[derive(Clone, Copy)]
pub union PACKAGE_VERSION_0 {
    pub Version: u64,
    pub Anonymous: PACKAGE_VERSION_0_0,
}
impl Default for PACKAGE_VERSION_0 {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PACKAGE_VERSION_0_0 {
    pub Revision: u16,
    pub Build: u16,
    pub Minor: u16,
    pub Major: u16,
}
pub type PCCERT_CONTEXT = *const CERT_CONTEXT;
pub type PCERT_EXTENSION = *mut CERT_EXTENSION;
pub type PCERT_INFO = *mut CERT_INFO;
pub const PKCS_7_ASN_ENCODING: i32 = 65536;
pub type PSID = *mut core::ffi::c_void;
pub type PackageDependencyLifetimeKind = i32;
pub const PackageDependencyLifetimeKind_FilePath: PackageDependencyLifetimeKind = 1;
pub const PackageDependencyLifetimeKind_Process: PackageDependencyLifetimeKind = 0;
pub const PackageDependencyLifetimeKind_RegistryKey: PackageDependencyLifetimeKind = 2;
pub type PackageDependencyProcessorArchitectures = u32;
pub const PackageDependencyProcessorArchitectures_Arm: PackageDependencyProcessorArchitectures = 8;
pub const PackageDependencyProcessorArchitectures_Arm64: PackageDependencyProcessorArchitectures =
    16;
pub const PackageDependencyProcessorArchitectures_Neutral: PackageDependencyProcessorArchitectures =
    1;
pub const PackageDependencyProcessorArchitectures_None: PackageDependencyProcessorArchitectures = 0;
pub const PackageDependencyProcessorArchitectures_X64: PackageDependencyProcessorArchitectures = 4;
pub const PackageDependencyProcessorArchitectures_X86: PackageDependencyProcessorArchitectures = 2;
pub const PackageDependencyProcessorArchitectures_X86A64: PackageDependencyProcessorArchitectures =
    32;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Panel(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Panel, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Panel, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Panel {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPanel>();
}
unsafe impl windows_core::Interface for Panel {
    type Vtable = <IPanel as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPanel as windows_core::Interface>::IID;
}
impl core::ops::Deref for Panel {
    type Target = IPanel;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Panel {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Panel";
}
unsafe impl Send for Panel {}
unsafe impl Sync for Panel {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PatternInterface(pub i32);
impl PatternInterface {
    pub const Invoke: Self = Self(0);
    pub const Selection: Self = Self(1);
    pub const Value: Self = Self(2);
    pub const RangeValue: Self = Self(3);
    pub const Scroll: Self = Self(4);
    pub const ScrollItem: Self = Self(5);
    pub const ExpandCollapse: Self = Self(6);
    pub const Grid: Self = Self(7);
    pub const GridItem: Self = Self(8);
    pub const MultipleView: Self = Self(9);
    pub const Window: Self = Self(10);
    pub const SelectionItem: Self = Self(11);
    pub const Dock: Self = Self(12);
    pub const Table: Self = Self(13);
    pub const TableItem: Self = Self(14);
    pub const Toggle: Self = Self(15);
    pub const Transform: Self = Self(16);
    pub const Text: Self = Self(17);
    pub const ItemContainer: Self = Self(18);
    pub const VirtualizedItem: Self = Self(19);
    pub const Text2: Self = Self(20);
    pub const TextChild: Self = Self(21);
    pub const TextRange: Self = Self(22);
    pub const Annotation: Self = Self(23);
    pub const Drag: Self = Self(24);
    pub const DropTarget: Self = Self(25);
    pub const ObjectModel: Self = Self(26);
    pub const Spreadsheet: Self = Self(27);
    pub const SpreadsheetItem: Self = Self(28);
    pub const Styles: Self = Self(29);
    pub const Transform2: Self = Self(30);
    pub const SynchronizedInput: Self = Self(31);
    pub const TextEdit: Self = Self(32);
    pub const CustomNavigation: Self = Self(33);
}
impl windows_core::imp::TypeKind for PatternInterface {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for PatternInterface {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Xaml.Automation.Peers.PatternInterface;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickFileResult(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PickFileResult,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for PickFileResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPickFileResult>();
}
unsafe impl windows_core::Interface for PickFileResult {
    type Vtable = <IPickFileResult as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPickFileResult as windows_core::Interface>::IID;
}
impl core::ops::Deref for PickFileResult {
    type Target = IPickFileResult;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for PickFileResult {
    const NAME: &'static str = "Microsoft.Windows.Storage.Pickers.PickFileResult";
}
unsafe impl Send for PickFileResult {}
unsafe impl Sync for PickFileResult {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PickFolderResult(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PickFolderResult,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for PickFolderResult {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPickFolderResult>();
}
unsafe impl windows_core::Interface for PickFolderResult {
    type Vtable = <IPickFolderResult as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPickFolderResult as windows_core::Interface>::IID;
}
impl core::ops::Deref for PickFolderResult {
    type Target = IPickFolderResult;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for PickFolderResult {
    const NAME: &'static str = "Microsoft.Windows.Storage.Pickers.PickFolderResult";
}
unsafe impl Send for PickFolderResult {}
unsafe impl Sync for PickFolderResult {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl windows_core::imp::TypeKind for Point {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Point {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.Point;f4;f4)");
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointInt32 {
    pub x: i32,
    pub y: i32,
}
impl windows_core::imp::TypeKind for PointInt32 {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for PointInt32 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Graphics.PointInt32;i4;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pointer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Pointer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for Pointer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointer>();
}
unsafe impl windows_core::Interface for Pointer {
    type Vtable = <IPointer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointer as windows_core::Interface>::IID;
}
impl core::ops::Deref for Pointer {
    type Target = IPointer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Pointer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.Pointer";
}
unsafe impl Send for Pointer {}
unsafe impl Sync for Pointer {}
windows_core::imp::define_interface!(
    PointerEventHandler,
    PointerEventHandler_Vtbl,
    0xa48a71e1_8bb4_5597_9e31_903a3f6a04fb
);
impl windows_core::RuntimeType for PointerEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct PointerEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct PointerEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<PointerRoutedEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<PointerRoutedEventArgs>)
        + 'static,
> PointerEventHandlerBox<F>
{
    const VTABLE: PointerEventHandler_Vtbl = PointerEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<PointerEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<PointerEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<PointerEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<PointerEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerPoint(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerPoint,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for PointerPoint {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerPoint>();
}
unsafe impl windows_core::Interface for PointerPoint {
    type Vtable = <IPointerPoint as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerPoint as windows_core::Interface>::IID;
}
impl core::ops::Deref for PointerPoint {
    type Target = IPointerPoint;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for PointerPoint {
    const NAME: &'static str = "Microsoft.UI.Input.PointerPoint";
}
unsafe impl Send for PointerPoint {}
unsafe impl Sync for PointerPoint {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerPointProperties(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerPointProperties,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for PointerPointProperties {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerPointProperties>();
}
unsafe impl windows_core::Interface for PointerPointProperties {
    type Vtable = <IPointerPointProperties as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerPointProperties as windows_core::Interface>::IID;
}
impl core::ops::Deref for PointerPointProperties {
    type Target = IPointerPointProperties;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for PointerPointProperties {
    const NAME: &'static str = "Microsoft.UI.Input.PointerPointProperties";
}
unsafe impl Send for PointerPointProperties {}
unsafe impl Sync for PointerPointProperties {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PointerRoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    PointerRoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(PointerRoutedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for PointerRoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPointerRoutedEventArgs>();
}
unsafe impl windows_core::Interface for PointerRoutedEventArgs {
    type Vtable = <IPointerRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPointerRoutedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for PointerRoutedEventArgs {
    type Target = IPointerRoutedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for PointerRoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Input.PointerRoutedEventArgs";
}
unsafe impl Send for PointerRoutedEventArgs {}
unsafe impl Sync for PointerRoutedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointerUpdateKind(pub i32);
impl PointerUpdateKind {
    pub const Other: Self = Self(0);
    pub const LeftButtonPressed: Self = Self(1);
    pub const LeftButtonReleased: Self = Self(2);
    pub const RightButtonPressed: Self = Self(3);
    pub const RightButtonReleased: Self = Self(4);
    pub const MiddleButtonPressed: Self = Self(5);
    pub const MiddleButtonReleased: Self = Self(6);
    pub const XButton1Pressed: Self = Self(7);
    pub const XButton1Released: Self = Self(8);
    pub const XButton2Pressed: Self = Self(9);
    pub const XButton2Released: Self = Self(10);
}
impl windows_core::imp::TypeKind for PointerUpdateKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for PointerUpdateKind {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Input.PointerUpdateKind;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Popup(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Popup, windows_core::IUnknown, windows_core::IInspectable);
windows_core::imp::required_hierarchy!(Popup, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for Popup {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IPopup>();
}
unsafe impl windows_core::Interface for Popup {
    type Vtable = <IPopup as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IPopup as windows_core::Interface>::IID;
}
impl core::ops::Deref for Popup {
    type Target = IPopup;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Popup {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.Popup";
}
unsafe impl Send for Popup {}
unsafe impl Sync for Popup {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressBar(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ProgressBar,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ProgressBar,
    RangeBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ProgressBar {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IProgressBar>();
}
unsafe impl windows_core::Interface for ProgressBar {
    type Vtable = <IProgressBar as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IProgressBar as windows_core::Interface>::IID;
}
impl core::ops::Deref for ProgressBar {
    type Target = IProgressBar;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ProgressBar {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ProgressBar";
}
unsafe impl Send for ProgressBar {}
unsafe impl Sync for ProgressBar {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProgressRing(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ProgressRing,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ProgressRing,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ProgressRing {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IProgressRing>();
}
unsafe impl windows_core::Interface for ProgressRing {
    type Vtable = <IProgressRing as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IProgressRing as windows_core::Interface>::IID;
}
impl core::ops::Deref for ProgressRing {
    type Target = IProgressRing;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ProgressRing {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ProgressRing";
}
unsafe impl Send for ProgressRing {}
unsafe impl Sync for ProgressRing {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
pub const RRF_RT_REG_SZ: i32 = 2;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RangeBase(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RangeBase,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    RangeBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for RangeBase {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRangeBase>();
}
unsafe impl windows_core::Interface for RangeBase {
    type Vtable = <IRangeBase as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRangeBase as windows_core::Interface>::IID;
}
impl core::ops::Deref for RangeBase {
    type Target = IRangeBase;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for RangeBase {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.RangeBase";
}
unsafe impl Send for RangeBase {}
unsafe impl Sync for RangeBase {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RectInt32 {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
impl windows_core::imp::TypeKind for RectInt32 {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for RectInt32 {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Windows.Graphics.RectInt32;i4;i4;i4;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceDictionary(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ResourceDictionary,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(ResourceDictionary, DependencyObject);
impl windows_core::RuntimeType for ResourceDictionary {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IResourceDictionary>();
}
unsafe impl windows_core::Interface for ResourceDictionary {
    type Vtable = <IResourceDictionary as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IResourceDictionary as windows_core::Interface>::IID;
}
impl core::ops::Deref for ResourceDictionary {
    type Target = IResourceDictionary;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ResourceDictionary {
    const NAME: &'static str = "Microsoft.UI.Xaml.ResourceDictionary";
}
unsafe impl Send for ResourceDictionary {}
unsafe impl Sync for ResourceDictionary {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoutedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    RoutedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for RoutedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IRoutedEventArgs>();
}
unsafe impl windows_core::Interface for RoutedEventArgs {
    type Vtable = <IRoutedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IRoutedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for RoutedEventArgs {
    type Target = IRoutedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for RoutedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.RoutedEventArgs";
}
unsafe impl Send for RoutedEventArgs {}
unsafe impl Sync for RoutedEventArgs {}
windows_core::imp::define_interface!(
    RoutedEventHandler,
    RoutedEventHandler_Vtbl,
    0xdae23d85_69ca_5bdf_805b_6161a3a215cc
);
impl windows_core::RuntimeType for RoutedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct RoutedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct RoutedEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<RoutedEventArgs>) + 'static,
> RoutedEventHandlerBox<F>
{
    const VTABLE: RoutedEventHandler_Vtbl = RoutedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<RoutedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<RoutedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
pub const SM_CXSMICON: i32 = 49;
pub const SM_CYSMICON: i32 = 50;
pub const SPI_GETCLIENTAREAANIMATION: i32 = 4162;
pub const STATEREPOSITORY_E_DEPENDENCY_NOT_RESOLVED: windows_core::HRESULT =
    windows_core::HRESULT(0x80670016_u32 as _);
pub const SW_SHOWNORMAL: i32 = 1;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScrollViewer(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ScrollViewer,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ScrollViewer,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ScrollViewer {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IScrollViewer>();
}
unsafe impl windows_core::Interface for ScrollViewer {
    type Vtable = <IScrollViewer as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IScrollViewer as windows_core::Interface>::IID;
}
impl core::ops::Deref for ScrollViewer {
    type Target = IScrollViewer;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ScrollViewer {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ScrollViewer";
}
unsafe impl Send for ScrollViewer {}
unsafe impl Sync for ScrollViewer {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SelectionChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(SelectionChangedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for SelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelectionChangedEventArgs>();
}
unsafe impl windows_core::Interface for SelectionChangedEventArgs {
    type Vtable = <ISelectionChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelectionChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for SelectionChangedEventArgs {
    type Target = ISelectionChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SelectionChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.SelectionChangedEventArgs";
}
unsafe impl Send for SelectionChangedEventArgs {}
unsafe impl Sync for SelectionChangedEventArgs {}
windows_core::imp::define_interface!(
    SelectionChangedEventHandler,
    SelectionChangedEventHandler_Vtbl,
    0xa232390d_0e34_595e_8931_fa928a9909f4
);
impl windows_core::RuntimeType for SelectionChangedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct SelectionChangedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct SelectionChangedEventHandlerBox<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<SelectionChangedEventArgs>,
        ) + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(
            windows_core::Ref<windows_core::IInspectable>,
            windows_core::Ref<SelectionChangedEventArgs>,
        ) + 'static,
> SelectionChangedEventHandlerBox<F>
{
    const VTABLE: SelectionChangedEventHandler_Vtbl = SelectionChangedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<SelectionChangedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<SelectionChangedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Selector(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Selector,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    Selector,
    ItemsControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for Selector {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelector>();
}
unsafe impl windows_core::Interface for Selector {
    type Vtable = <ISelector as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelector as windows_core::Interface>::IID;
}
impl core::ops::Deref for Selector {
    type Target = ISelector;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Selector {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.Selector";
}
unsafe impl Send for Selector {}
unsafe impl Sync for Selector {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectorItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SelectorItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for SelectorItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISelectorItem>();
}
unsafe impl windows_core::Interface for SelectorItem {
    type Vtable = <ISelectorItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISelectorItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for SelectorItem {
    type Target = ISelectorItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SelectorItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.SelectorItem";
}
unsafe impl Send for SelectorItem {}
unsafe impl Sync for SelectorItem {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
impl windows_core::imp::TypeKind for Size {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Size {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Foundation.Size;f4;f4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SizeChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SizeChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(SizeChangedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for SizeChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISizeChangedEventArgs>();
}
unsafe impl windows_core::Interface for SizeChangedEventArgs {
    type Vtable = <ISizeChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISizeChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for SizeChangedEventArgs {
    type Target = ISizeChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SizeChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.SizeChangedEventArgs";
}
unsafe impl Send for SizeChangedEventArgs {}
unsafe impl Sync for SizeChangedEventArgs {}
windows_core::imp::define_interface!(
    SizeChangedEventHandler,
    SizeChangedEventHandler_Vtbl,
    0x8d7b1a58_14c6_51c9_892c_9fcce368e77d
);
impl windows_core::RuntimeType for SizeChangedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct SizeChangedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct SizeChangedEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<SizeChangedEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<SizeChangedEventArgs>)
        + 'static,
> SizeChangedEventHandlerBox<F>
{
    const VTABLE: SizeChangedEventHandler_Vtbl = SizeChangedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<SizeChangedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<SizeChangedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<SizeChangedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<SizeChangedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SizeInt32 {
    pub width: i32,
    pub height: i32,
}
impl windows_core::imp::TypeKind for SizeInt32 {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for SizeInt32 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Windows.Graphics.SizeInt32;i4;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SoftwareBitmap(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SoftwareBitmap,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl SoftwareBitmap {
    pub fn CreateCopyWithAlphaFromBuffer<P0>(
        source: P0,
        format: BitmapPixelFormat,
        width: i32,
        height: i32,
        alpha: BitmapAlphaMode,
    ) -> windows_core::Result<Self>
    where
        P0: windows_core::Param<IBuffer>,
    {
        Self::ISoftwareBitmapStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateCopyWithAlphaFromBuffer)(
                windows_core::Interface::as_raw(this),
                source.param().abi(),
                format,
                width,
                height,
                alpha,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn CreateCopyFromSurfaceAsync<P0>(
        surface: P0,
    ) -> windows_core::Result<windows_future::IAsyncOperation<Self>>
    where
        P0: windows_core::Param<IDirect3DSurface>,
    {
        Self::ISoftwareBitmapStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateCopyFromSurfaceAsync)(
                windows_core::Interface::as_raw(this),
                surface.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ISoftwareBitmapStatics<R, F: FnOnce(&ISoftwareBitmapStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<SoftwareBitmap, ISoftwareBitmapStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for SoftwareBitmap {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISoftwareBitmap>();
}
unsafe impl windows_core::Interface for SoftwareBitmap {
    type Vtable = <ISoftwareBitmap as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISoftwareBitmap as windows_core::Interface>::IID;
}
impl core::ops::Deref for SoftwareBitmap {
    type Target = ISoftwareBitmap;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SoftwareBitmap {
    const NAME: &'static str = "Windows.Graphics.Imaging.SoftwareBitmap";
}
unsafe impl Send for SoftwareBitmap {}
unsafe impl Sync for SoftwareBitmap {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Storyboard(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Storyboard,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Storyboard, Timeline, DependencyObject);
impl windows_core::RuntimeType for Storyboard {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IStoryboard>();
}
unsafe impl windows_core::Interface for Storyboard {
    type Vtable = <IStoryboard as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IStoryboard as windows_core::Interface>::IID;
}
impl core::ops::Deref for Storyboard {
    type Target = IStoryboard;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Storyboard {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Animation.Storyboard";
}
unsafe impl Send for Storyboard {}
unsafe impl Sync for Storyboard {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemBackdrop(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    SystemBackdrop,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(SystemBackdrop, DependencyObject);
impl windows_core::RuntimeType for SystemBackdrop {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ISystemBackdrop>();
}
unsafe impl windows_core::Interface for SystemBackdrop {
    type Vtable = <ISystemBackdrop as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ISystemBackdrop as windows_core::Interface>::IID;
}
impl core::ops::Deref for SystemBackdrop {
    type Target = ISystemBackdrop;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for SystemBackdrop {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.SystemBackdrop";
}
unsafe impl Send for SystemBackdrop {}
unsafe impl Sync for SystemBackdrop {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TabView,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for TabView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabView>();
}
unsafe impl windows_core::Interface for TabView {
    type Vtable = <ITabView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITabView as windows_core::Interface>::IID;
}
impl core::ops::Deref for TabView {
    type Target = ITabView;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TabView {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabView";
}
unsafe impl Send for TabView {}
unsafe impl Sync for TabView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TabViewItem,
    ListViewItem,
    SelectorItem,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl TabViewItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::ITabViewItemFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ITabViewItemFactory<R, F: FnOnce(&ITabViewItemFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TabViewItem, ITabViewItemFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TabViewItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewItem>();
}
unsafe impl windows_core::Interface for TabViewItem {
    type Vtable = <ITabViewItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITabViewItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for TabViewItem {
    type Target = ITabViewItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TabViewItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewItem";
}
unsafe impl Send for TabViewItem {}
unsafe impl Sync for TabViewItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TabViewTabCloseRequestedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TabViewTabCloseRequestedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for TabViewTabCloseRequestedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITabViewTabCloseRequestedEventArgs>();
}
unsafe impl windows_core::Interface for TabViewTabCloseRequestedEventArgs {
    type Vtable = <ITabViewTabCloseRequestedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITabViewTabCloseRequestedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TabViewTabCloseRequestedEventArgs {
    type Target = ITabViewTabCloseRequestedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TabViewTabCloseRequestedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TabViewTabCloseRequestedEventArgs";
}
unsafe impl Send for TabViewTabCloseRequestedEventArgs {}
unsafe impl Sync for TabViewTabCloseRequestedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextAlignment(pub i32);
impl TextAlignment {
    pub const Center: Self = Self(0);
    pub const Left: Self = Self(1);
    pub const Start: Self = Self(1);
    pub const Right: Self = Self(2);
    pub const End: Self = Self(2);
    pub const Justify: Self = Self(3);
    pub const DetectFromContent: Self = Self(4);
}
impl windows_core::imp::TypeKind for TextAlignment {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TextAlignment {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.TextAlignment;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBlock(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBlock,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextBlock, FrameworkElement, UIElement, DependencyObject);
impl windows_core::RuntimeType for TextBlock {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBlock>();
}
unsafe impl windows_core::Interface for TextBlock {
    type Vtable = <ITextBlock as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBlock as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextBlock {
    type Target = ITextBlock;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextBlock {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBlock";
}
unsafe impl Send for TextBlock {}
unsafe impl Sync for TextBlock {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextBox(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextBox,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TextBox,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for TextBox {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextBox>();
}
unsafe impl windows_core::Interface for TextBox {
    type Vtable = <ITextBox as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextBox as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextBox {
    type Target = ITextBox;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextBox {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextBox";
}
unsafe impl Send for TextBox {}
unsafe impl Sync for TextBox {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TextChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TextChangedEventArgs, RoutedEventArgs);
impl windows_core::RuntimeType for TextChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITextChangedEventArgs>();
}
unsafe impl windows_core::Interface for TextChangedEventArgs {
    type Vtable = <ITextChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITextChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TextChangedEventArgs {
    type Target = ITextChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TextChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TextChangedEventArgs";
}
unsafe impl Send for TextChangedEventArgs {}
unsafe impl Sync for TextChangedEventArgs {}
windows_core::imp::define_interface!(
    TextChangedEventHandler,
    TextChangedEventHandler_Vtbl,
    0x5d8ddcff_45d8_5e7c_9b8b_c41d2893c6a1
);
impl windows_core::RuntimeType for TextChangedEventHandler {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_interface::<Self>();
}
#[repr(C)]
pub struct TextChangedEventHandler_Vtbl {
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT,
}
struct TextChangedEventHandlerBox<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<TextChangedEventArgs>)
        + 'static,
>(core::marker::PhantomData<(fn() -> F,)>);
impl<
    F: Fn(windows_core::Ref<windows_core::IInspectable>, windows_core::Ref<TextChangedEventArgs>)
        + 'static,
> TextChangedEventHandlerBox<F>
{
    const VTABLE: TextChangedEventHandler_Vtbl = TextChangedEventHandler_Vtbl {
        base__: windows_core::IUnknown_Vtbl {
            QueryInterface:
                windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::QueryInterface,
            AddRef: windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::AddRef,
            Release: windows_core::imp::DelegateBox::<TextChangedEventHandler, F>::Release,
        },
        Invoke: Self::Invoke,
    };
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: *mut core::ffi::c_void,
        e: *mut core::ffi::c_void,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<TextChangedEventHandler, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&e),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeline(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    Timeline,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(Timeline, DependencyObject);
impl windows_core::RuntimeType for Timeline {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITimeline>();
}
unsafe impl windows_core::Interface for Timeline {
    type Vtable = <ITimeline as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITimeline as windows_core::Interface>::IID;
}
impl core::ops::Deref for Timeline {
    type Target = ITimeline;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Timeline {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.Animation.Timeline";
}
unsafe impl Send for Timeline {}
unsafe impl Sync for Timeline {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TitleBarTheme(pub i32);
impl TitleBarTheme {
    pub const Legacy: Self = Self(0);
    pub const UseDefaultAppMode: Self = Self(1);
    pub const Light: Self = Self(2);
    pub const Dark: Self = Self(3);
}
impl windows_core::imp::TypeKind for TitleBarTheme {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TitleBarTheme {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"enum(Microsoft.UI.Windowing.TitleBarTheme;i4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleButton(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ToggleButton,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ToggleButton,
    ButtonBase,
    ContentControl,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ToggleButton {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToggleButton>();
}
unsafe impl windows_core::Interface for ToggleButton {
    type Vtable = <IToggleButton as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IToggleButton as windows_core::Interface>::IID;
}
impl core::ops::Deref for ToggleButton {
    type Target = IToggleButton;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ToggleButton {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.Primitives.ToggleButton";
}
unsafe impl Send for ToggleButton {}
unsafe impl Sync for ToggleButton {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleMenuFlyoutItem(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ToggleMenuFlyoutItem,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ToggleMenuFlyoutItem,
    MenuFlyoutItem,
    MenuFlyoutItemBase,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl ToggleMenuFlyoutItem {
    pub fn new() -> windows_core::Result<Self> {
        Self::IToggleMenuFlyoutItemFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IToggleMenuFlyoutItemFactory<
        R,
        F: FnOnce(&IToggleMenuFlyoutItemFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            ToggleMenuFlyoutItem,
            IToggleMenuFlyoutItemFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ToggleMenuFlyoutItem {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToggleMenuFlyoutItem>();
}
unsafe impl windows_core::Interface for ToggleMenuFlyoutItem {
    type Vtable = <IToggleMenuFlyoutItem as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IToggleMenuFlyoutItem as windows_core::Interface>::IID;
}
impl core::ops::Deref for ToggleMenuFlyoutItem {
    type Target = IToggleMenuFlyoutItem;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ToggleMenuFlyoutItem {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ToggleMenuFlyoutItem";
}
unsafe impl Send for ToggleMenuFlyoutItem {}
unsafe impl Sync for ToggleMenuFlyoutItem {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToggleSwitch(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ToggleSwitch,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    ToggleSwitch,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for ToggleSwitch {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToggleSwitch>();
}
unsafe impl windows_core::Interface for ToggleSwitch {
    type Vtable = <IToggleSwitch as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IToggleSwitch as windows_core::Interface>::IID;
}
impl core::ops::Deref for ToggleSwitch {
    type Target = IToggleSwitch;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ToggleSwitch {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ToggleSwitch";
}
unsafe impl Send for ToggleSwitch {}
unsafe impl Sync for ToggleSwitch {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolTipService(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    ToolTipService,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl ToolTipService {
    pub fn SetToolTip<P0, P1>(element: P0, value: P1) -> windows_core::Result<()>
    where
        P0: windows_core::Param<DependencyObject>,
        P1: windows_core::Param<windows_core::IInspectable>,
    {
        Self::IToolTipServiceStatics(|this| unsafe {
            (windows_core::Interface::vtable(this).SetToolTip)(
                windows_core::Interface::as_raw(this),
                element.param().abi(),
                value.param().abi(),
            )
            .ok()
        })
    }
    fn IToolTipServiceStatics<R, F: FnOnce(&IToolTipServiceStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<ToolTipService, IToolTipServiceStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for ToolTipService {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IToolTipService>();
}
unsafe impl windows_core::Interface for ToolTipService {
    type Vtable = <IToolTipService as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IToolTipService as windows_core::Interface>::IID;
}
impl core::ops::Deref for ToolTipService {
    type Target = IToolTipService;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for ToolTipService {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.ToolTipService";
}
unsafe impl Send for ToolTipService {}
unsafe impl Sync for ToolTipService {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeView(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeView,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(
    TreeView,
    Control,
    FrameworkElement,
    UIElement,
    DependencyObject
);
impl windows_core::RuntimeType for TreeView {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeView>();
}
unsafe impl windows_core::Interface for TreeView {
    type Vtable = <ITreeView as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITreeView as windows_core::Interface>::IID;
}
impl core::ops::Deref for TreeView {
    type Target = ITreeView;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TreeView {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeView";
}
unsafe impl Send for TreeView {}
unsafe impl Sync for TreeView {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewDragItemsCompletedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewDragItemsCompletedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for TreeViewDragItemsCompletedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewDragItemsCompletedEventArgs>();
}
unsafe impl windows_core::Interface for TreeViewDragItemsCompletedEventArgs {
    type Vtable = <ITreeViewDragItemsCompletedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITreeViewDragItemsCompletedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TreeViewDragItemsCompletedEventArgs {
    type Target = ITreeViewDragItemsCompletedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TreeViewDragItemsCompletedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewDragItemsCompletedEventArgs";
}
unsafe impl Send for TreeViewDragItemsCompletedEventArgs {}
unsafe impl Sync for TreeViewDragItemsCompletedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewDragItemsStartingEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewDragItemsStartingEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for TreeViewDragItemsStartingEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewDragItemsStartingEventArgs>();
}
unsafe impl windows_core::Interface for TreeViewDragItemsStartingEventArgs {
    type Vtable = <ITreeViewDragItemsStartingEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITreeViewDragItemsStartingEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TreeViewDragItemsStartingEventArgs {
    type Target = ITreeViewDragItemsStartingEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TreeViewDragItemsStartingEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewDragItemsStartingEventArgs";
}
unsafe impl Send for TreeViewDragItemsStartingEventArgs {}
unsafe impl Sync for TreeViewDragItemsStartingEventArgs {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewNode(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewNode,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(TreeViewNode, DependencyObject);
impl TreeViewNode {
    pub fn new() -> windows_core::Result<Self> {
        Self::ITreeViewNodeFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn ITreeViewNodeFactory<R, F: FnOnce(&ITreeViewNodeFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<TreeViewNode, ITreeViewNodeFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for TreeViewNode {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewNode>();
}
unsafe impl windows_core::Interface for TreeViewNode {
    type Vtable = <ITreeViewNode as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <ITreeViewNode as windows_core::Interface>::IID;
}
impl core::ops::Deref for TreeViewNode {
    type Target = ITreeViewNode;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TreeViewNode {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewNode";
}
unsafe impl Send for TreeViewNode {}
unsafe impl Sync for TreeViewNode {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeViewSelectionChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    TreeViewSelectionChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for TreeViewSelectionChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, ITreeViewSelectionChangedEventArgs>();
}
unsafe impl windows_core::Interface for TreeViewSelectionChangedEventArgs {
    type Vtable = <ITreeViewSelectionChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <ITreeViewSelectionChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for TreeViewSelectionChangedEventArgs {
    type Target = ITreeViewSelectionChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for TreeViewSelectionChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.TreeViewSelectionChangedEventArgs";
}
unsafe impl Send for TreeViewSelectionChangedEventArgs {}
unsafe impl Sync for TreeViewSelectionChangedEventArgs {}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TypeKind(pub i32);
impl TypeKind {
    pub const Primitive: Self = Self(0);
    pub const Metadata: Self = Self(1);
    pub const Custom: Self = Self(2);
}
impl windows_core::imp::TypeKind for TypeKind {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for TypeKind {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.UI.Xaml.Interop.TypeKind;i4)");
}
#[repr(C)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TypeName {
    pub name: windows_core::HSTRING,
    pub kind: TypeKind,
}
impl windows_core::imp::TypeKind for TypeName {
    type TypeKind = windows_core::imp::CloneType;
}
impl windows_core::RuntimeType for TypeName {
    const SIGNATURE : windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice (b"struct(Windows.UI.Xaml.Interop.TypeName;string;enum(Windows.UI.Xaml.Interop.TypeKind;i4))") ;
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedEventHandler<TSender, TResult>(
    windows_core::IUnknown,
    core::marker::PhantomData<TSender>,
    core::marker::PhantomData<TResult>,
)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
unsafe impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
> windows_core::Interface for TypedEventHandler<TSender, TResult>
{
    type Vtable = TypedEventHandler_Vtbl<TSender, TResult>;
    const IID: windows_core::GUID =
        windows_core::GUID::from_signature(<Self as windows_core::RuntimeType>::SIGNATURE);
}
impl<TSender: windows_core::RuntimeType + 'static, TResult: windows_core::RuntimeType + 'static>
    windows_core::RuntimeType for TypedEventHandler<TSender, TResult>
{
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::new()
        .push_slice(b"pinterface({9de1c534-6ae1-11e0-84e1-18a905bcc53f}")
        .push_slice(b";")
        .push_other(TSender::SIGNATURE)
        .push_slice(b";")
        .push_other(TResult::SIGNATURE)
        .push_slice(b")");
}
impl<TSender: windows_core::RuntimeType + 'static, TResult: windows_core::RuntimeType + 'static>
    TypedEventHandler<TSender, TResult>
{
    pub fn new<F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) + 'static>(
        invoke: F,
    ) -> Self {
        let com = windows_core::imp::DelegateBox::<Self, F>::new(
            &TypedEventHandlerBox::<TSender, TResult, F>::VTABLE,
            invoke,
        );
        unsafe { core::mem::transmute(windows_core::imp::box_new(com)) }
    }
}
#[repr(C)]
pub struct TypedEventHandler_Vtbl<TSender, TResult>
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
{
    base__: windows_core::IUnknown_Vtbl,
    Invoke: unsafe extern "system" fn(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT,
    TSender: core::marker::PhantomData<TSender>,
    TResult: core::marker::PhantomData<TResult>,
}
struct TypedEventHandlerBox<
    TSender,
    TResult,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) + 'static,
>(core::marker::PhantomData<(TSender, TResult, fn() -> F)>)
where
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static;
impl<
    TSender: windows_core::RuntimeType + 'static,
    TResult: windows_core::RuntimeType + 'static,
    F: Fn(windows_core::Ref<TSender>, windows_core::Ref<TResult>) + 'static,
> TypedEventHandlerBox<TSender, TResult, F>
{
    const VTABLE : TypedEventHandler_Vtbl < TSender , TResult , > = TypedEventHandler_Vtbl::< TSender , TResult , > { base__ : windows_core::IUnknown_Vtbl { QueryInterface : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::QueryInterface , AddRef : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::AddRef , Release : windows_core::imp::DelegateBox::< TypedEventHandler < TSender , TResult > , F >::Release , } , Invoke : Self::Invoke , TSender : core::marker::PhantomData::< TSender > , TResult : core::marker::PhantomData::< TResult > } ;
    unsafe extern "system" fn Invoke(
        this: *mut core::ffi::c_void,
        sender: windows_core::imp::AbiType<TSender>,
        args: windows_core::imp::AbiType<TResult>,
    ) -> windows_core::HRESULT {
        unsafe {
            let this = &mut *(this as *mut *mut core::ffi::c_void
                as *mut windows_core::imp::DelegateBox<TypedEventHandler<TSender, TResult>, F>);
            (this.invoke)(
                core::mem::transmute_copy(&sender),
                core::mem::transmute_copy(&args),
            );
            windows_core::HRESULT(0)
        }
    }
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UIElement(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    UIElement,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(UIElement, DependencyObject);
impl windows_core::RuntimeType for UIElement {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IUIElement>();
}
unsafe impl windows_core::Interface for UIElement {
    type Vtable = <IUIElement as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IUIElement as windows_core::Interface>::IID;
}
impl core::ops::Deref for UIElement {
    type Target = IUIElement;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for UIElement {
    const NAME: &'static str = "Microsoft.UI.Xaml.UIElement";
}
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UIElementCollection(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    UIElementCollection,
    windows_core::IUnknown,
    windows_core::IInspectable,
    windows_collections::IVector<UIElement>
);
impl windows_core::RuntimeType for UIElementCollection {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, windows_collections::IVector<UIElement>>(
        );
}
unsafe impl windows_core::Interface for UIElementCollection {
    type Vtable = <windows_collections::IVector<UIElement> as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID =
        <windows_collections::IVector<UIElement> as windows_core::Interface>::IID;
}
impl core::ops::Deref for UIElementCollection {
    type Target = windows_collections::IVector<UIElement>;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for UIElementCollection {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.UIElementCollection";
}
unsafe impl Send for UIElementCollection {}
unsafe impl Sync for UIElementCollection {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Uri(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Uri, windows_core::IUnknown, windows_core::IInspectable);
impl Uri {
    pub fn CreateUri(uri: &str) -> windows_core::Result<Self> {
        Self::IUriRuntimeClassFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateUri)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(uri)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IUriRuntimeClassFactory<
        R,
        F: FnOnce(&IUriRuntimeClassFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Uri, IUriRuntimeClassFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Uri {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IUriRuntimeClass>();
}
unsafe impl windows_core::Interface for Uri {
    type Vtable = <IUriRuntimeClass as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IUriRuntimeClass as windows_core::Interface>::IID;
}
impl core::ops::Deref for Uri {
    type Target = IUriRuntimeClass;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Uri {
    const NAME: &'static str = "Windows.Foundation.Uri";
}
unsafe impl Send for Uri {}
unsafe impl Sync for Uri {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl windows_core::imp::TypeKind for Vector3 {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Vector3 {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Windows.Foundation.Numerics.Vector3;f4;f4;f4)",
    );
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VirtualKey(pub i32);
impl VirtualKey {
    pub const None: Self = Self(0);
    pub const LeftButton: Self = Self(1);
    pub const RightButton: Self = Self(2);
    pub const Cancel: Self = Self(3);
    pub const MiddleButton: Self = Self(4);
    pub const XButton1: Self = Self(5);
    pub const XButton2: Self = Self(6);
    pub const Back: Self = Self(8);
    pub const Tab: Self = Self(9);
    pub const Clear: Self = Self(12);
    pub const Enter: Self = Self(13);
    pub const Shift: Self = Self(16);
    pub const Control: Self = Self(17);
    pub const Menu: Self = Self(18);
    pub const Pause: Self = Self(19);
    pub const CapitalLock: Self = Self(20);
    pub const Kana: Self = Self(21);
    pub const Hangul: Self = Self(21);
    pub const ImeOn: Self = Self(22);
    pub const Junja: Self = Self(23);
    pub const Final: Self = Self(24);
    pub const Hanja: Self = Self(25);
    pub const Kanji: Self = Self(25);
    pub const ImeOff: Self = Self(26);
    pub const Escape: Self = Self(27);
    pub const Convert: Self = Self(28);
    pub const NonConvert: Self = Self(29);
    pub const Accept: Self = Self(30);
    pub const ModeChange: Self = Self(31);
    pub const Space: Self = Self(32);
    pub const PageUp: Self = Self(33);
    pub const PageDown: Self = Self(34);
    pub const End: Self = Self(35);
    pub const Home: Self = Self(36);
    pub const Left: Self = Self(37);
    pub const Up: Self = Self(38);
    pub const Right: Self = Self(39);
    pub const Down: Self = Self(40);
    pub const Select: Self = Self(41);
    pub const Print: Self = Self(42);
    pub const Execute: Self = Self(43);
    pub const Snapshot: Self = Self(44);
    pub const Insert: Self = Self(45);
    pub const Delete: Self = Self(46);
    pub const Help: Self = Self(47);
    pub const Number0: Self = Self(48);
    pub const Number1: Self = Self(49);
    pub const Number2: Self = Self(50);
    pub const Number3: Self = Self(51);
    pub const Number4: Self = Self(52);
    pub const Number5: Self = Self(53);
    pub const Number6: Self = Self(54);
    pub const Number7: Self = Self(55);
    pub const Number8: Self = Self(56);
    pub const Number9: Self = Self(57);
    pub const A: Self = Self(65);
    pub const B: Self = Self(66);
    pub const C: Self = Self(67);
    pub const D: Self = Self(68);
    pub const E: Self = Self(69);
    pub const F: Self = Self(70);
    pub const G: Self = Self(71);
    pub const H: Self = Self(72);
    pub const I: Self = Self(73);
    pub const J: Self = Self(74);
    pub const K: Self = Self(75);
    pub const L: Self = Self(76);
    pub const M: Self = Self(77);
    pub const N: Self = Self(78);
    pub const O: Self = Self(79);
    pub const P: Self = Self(80);
    pub const Q: Self = Self(81);
    pub const R: Self = Self(82);
    pub const S: Self = Self(83);
    pub const T: Self = Self(84);
    pub const U: Self = Self(85);
    pub const V: Self = Self(86);
    pub const W: Self = Self(87);
    pub const X: Self = Self(88);
    pub const Y: Self = Self(89);
    pub const Z: Self = Self(90);
    pub const LeftWindows: Self = Self(91);
    pub const RightWindows: Self = Self(92);
    pub const Application: Self = Self(93);
    pub const Sleep: Self = Self(95);
    pub const NumberPad0: Self = Self(96);
    pub const NumberPad1: Self = Self(97);
    pub const NumberPad2: Self = Self(98);
    pub const NumberPad3: Self = Self(99);
    pub const NumberPad4: Self = Self(100);
    pub const NumberPad5: Self = Self(101);
    pub const NumberPad6: Self = Self(102);
    pub const NumberPad7: Self = Self(103);
    pub const NumberPad8: Self = Self(104);
    pub const NumberPad9: Self = Self(105);
    pub const Multiply: Self = Self(106);
    pub const Add: Self = Self(107);
    pub const Separator: Self = Self(108);
    pub const Subtract: Self = Self(109);
    pub const Decimal: Self = Self(110);
    pub const Divide: Self = Self(111);
    pub const F1: Self = Self(112);
    pub const F2: Self = Self(113);
    pub const F3: Self = Self(114);
    pub const F4: Self = Self(115);
    pub const F5: Self = Self(116);
    pub const F6: Self = Self(117);
    pub const F7: Self = Self(118);
    pub const F8: Self = Self(119);
    pub const F9: Self = Self(120);
    pub const F10: Self = Self(121);
    pub const F11: Self = Self(122);
    pub const F12: Self = Self(123);
    pub const F13: Self = Self(124);
    pub const F14: Self = Self(125);
    pub const F15: Self = Self(126);
    pub const F16: Self = Self(127);
    pub const F17: Self = Self(128);
    pub const F18: Self = Self(129);
    pub const F19: Self = Self(130);
    pub const F20: Self = Self(131);
    pub const F21: Self = Self(132);
    pub const F22: Self = Self(133);
    pub const F23: Self = Self(134);
    pub const F24: Self = Self(135);
    pub const NavigationView: Self = Self(136);
    pub const NavigationMenu: Self = Self(137);
    pub const NavigationUp: Self = Self(138);
    pub const NavigationDown: Self = Self(139);
    pub const NavigationLeft: Self = Self(140);
    pub const NavigationRight: Self = Self(141);
    pub const NavigationAccept: Self = Self(142);
    pub const NavigationCancel: Self = Self(143);
    pub const NumberKeyLock: Self = Self(144);
    pub const Scroll: Self = Self(145);
    pub const LeftShift: Self = Self(160);
    pub const RightShift: Self = Self(161);
    pub const LeftControl: Self = Self(162);
    pub const RightControl: Self = Self(163);
    pub const LeftMenu: Self = Self(164);
    pub const RightMenu: Self = Self(165);
    pub const GoBack: Self = Self(166);
    pub const GoForward: Self = Self(167);
    pub const Refresh: Self = Self(168);
    pub const Stop: Self = Self(169);
    pub const Search: Self = Self(170);
    pub const Favorites: Self = Self(171);
    pub const GoHome: Self = Self(172);
    pub const GamepadA: Self = Self(195);
    pub const GamepadB: Self = Self(196);
    pub const GamepadX: Self = Self(197);
    pub const GamepadY: Self = Self(198);
    pub const GamepadRightShoulder: Self = Self(199);
    pub const GamepadLeftShoulder: Self = Self(200);
    pub const GamepadLeftTrigger: Self = Self(201);
    pub const GamepadRightTrigger: Self = Self(202);
    pub const GamepadDPadUp: Self = Self(203);
    pub const GamepadDPadDown: Self = Self(204);
    pub const GamepadDPadLeft: Self = Self(205);
    pub const GamepadDPadRight: Self = Self(206);
    pub const GamepadMenu: Self = Self(207);
    pub const GamepadView: Self = Self(208);
    pub const GamepadLeftThumbstickButton: Self = Self(209);
    pub const GamepadRightThumbstickButton: Self = Self(210);
    pub const GamepadLeftThumbstickUp: Self = Self(211);
    pub const GamepadLeftThumbstickDown: Self = Self(212);
    pub const GamepadLeftThumbstickRight: Self = Self(213);
    pub const GamepadLeftThumbstickLeft: Self = Self(214);
    pub const GamepadRightThumbstickUp: Self = Self(215);
    pub const GamepadRightThumbstickDown: Self = Self(216);
    pub const GamepadRightThumbstickRight: Self = Self(217);
    pub const GamepadRightThumbstickLeft: Self = Self(218);
}
impl windows_core::imp::TypeKind for VirtualKey {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for VirtualKey {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.System.VirtualKey;i4)");
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VirtualKeyModifiers(pub u32);
impl VirtualKeyModifiers {
    pub const None: Self = Self(0);
    pub const Control: Self = Self(1);
    pub const Menu: Self = Self(2);
    pub const Shift: Self = Self(4);
    pub const Windows: Self = Self(8);
}
impl windows_core::imp::TypeKind for VirtualKeyModifiers {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for VirtualKeyModifiers {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Windows.System.VirtualKeyModifiers;u4)");
}
impl VirtualKeyModifiers {
    pub const fn contains(&self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}
impl core::ops::BitOr for VirtualKeyModifiers {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}
impl core::ops::BitAnd for VirtualKeyModifiers {
    type Output = Self;
    fn bitand(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }
}
impl core::ops::BitOrAssign for VirtualKeyModifiers {
    fn bitor_assign(&mut self, other: Self) {
        self.0.bitor_assign(other.0);
    }
}
impl core::ops::BitAndAssign for VirtualKeyModifiers {
    fn bitand_assign(&mut self, other: Self) {
        self.0.bitand_assign(other.0);
    }
}
impl core::ops::Not for VirtualKeyModifiers {
    type Output = Self;
    fn not(self) -> Self {
        Self(self.0.not())
    }
}
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Visibility(pub i32);
impl Visibility {
    pub const Visible: Self = Self(0);
    pub const Collapsed: Self = Self(1);
}
impl windows_core::imp::TypeKind for Visibility {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for Visibility {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"enum(Microsoft.UI.Xaml.Visibility;i4)");
}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VisualTreeHelper(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    VisualTreeHelper,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl VisualTreeHelper {
    pub fn GetChild<P0>(reference: P0, childindex: i32) -> windows_core::Result<DependencyObject>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetChild)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                childindex,
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    pub fn GetChildrenCount<P0>(reference: P0) -> windows_core::Result<i32>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetChildrenCount)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                &mut result__,
            )
            .map(|| result__)
        })
    }
    pub fn GetParent<P0>(reference: P0) -> windows_core::Result<DependencyObject>
    where
        P0: windows_core::Param<DependencyObject>,
    {
        Self::IVisualTreeHelperStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).GetParent)(
                windows_core::Interface::as_raw(this),
                reference.param().abi(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IVisualTreeHelperStatics<
        R,
        F: FnOnce(&IVisualTreeHelperStatics) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<VisualTreeHelper, IVisualTreeHelperStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for VisualTreeHelper {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IVisualTreeHelper>();
}
unsafe impl windows_core::Interface for VisualTreeHelper {
    type Vtable = <IVisualTreeHelper as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IVisualTreeHelper as windows_core::Interface>::IID;
}
impl core::ops::Deref for VisualTreeHelper {
    type Target = IVisualTreeHelper;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for VisualTreeHelper {
    const NAME: &'static str = "Microsoft.UI.Xaml.Media.VisualTreeHelper";
}
unsafe impl Send for VisualTreeHelper {}
unsafe impl Sync for VisualTreeHelper {}
pub const WM_CLOSE: i32 = 16;
pub const WM_SETICON: i32 = 128;
pub type WNDENUMPROC =
    Option<unsafe extern "system" fn(param0: HWND, param1: LPARAM) -> windows_core::BOOL>;
pub type WPARAM = usize;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebView2(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WebView2,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(WebView2, FrameworkElement, UIElement, DependencyObject);
impl WebView2 {
    pub fn new() -> windows_core::Result<Self> {
        Self::IWebView2Factory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IWebView2Factory<R, F: FnOnce(&IWebView2Factory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<WebView2, IWebView2Factory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for WebView2 {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWebView2>();
}
unsafe impl windows_core::Interface for WebView2 {
    type Vtable = <IWebView2 as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWebView2 as windows_core::Interface>::IID;
}
impl core::ops::Deref for WebView2 {
    type Target = IWebView2;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for WebView2 {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.WebView2";
}
unsafe impl Send for WebView2 {}
unsafe impl Sync for WebView2 {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Window(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(Window, windows_core::IUnknown, windows_core::IInspectable);
impl Window {
    pub fn new() -> windows_core::Result<Self> {
        Self::IWindowFactory(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).CreateInstance)(
                windows_core::Interface::as_raw(this),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IWindowFactory<R, F: FnOnce(&IWindowFactory) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<Window, IWindowFactory> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for Window {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindow>();
}
unsafe impl windows_core::Interface for Window {
    type Vtable = <IWindow as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindow as windows_core::Interface>::IID;
}
impl core::ops::Deref for Window {
    type Target = IWindow;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for Window {
    const NAME: &'static str = "Microsoft.UI.Xaml.Window";
}
unsafe impl Send for Window {}
unsafe impl Sync for Window {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    WindowEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for WindowEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IWindowEventArgs>();
}
unsafe impl windows_core::Interface for WindowEventArgs {
    type Vtable = <IWindowEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IWindowEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for WindowEventArgs {
    type Target = IWindowEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for WindowEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.WindowEventArgs";
}
unsafe impl Send for WindowEventArgs {}
unsafe impl Sync for WindowEventArgs {}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowId {
    pub value: u64,
}
impl windows_core::imp::TypeKind for WindowId {
    type TypeKind = windows_core::imp::CopyType;
}
impl windows_core::RuntimeType for WindowId {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::from_slice(b"struct(Microsoft.UI.WindowId;u8)");
}
pub const X509_ASN_ENCODING: i32 = 1;
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlControlsResources(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlControlsResources,
    windows_core::IUnknown,
    windows_core::IInspectable
);
windows_core::imp::required_hierarchy!(XamlControlsResources, ResourceDictionary, DependencyObject);
impl XamlControlsResources {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            XamlControlsResources,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlControlsResources {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlControlsResources>();
}
unsafe impl windows_core::Interface for XamlControlsResources {
    type Vtable = <IXamlControlsResources as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlControlsResources as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlControlsResources {
    type Target = IXamlControlsResources;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlControlsResources {
    const NAME: &'static str = "Microsoft.UI.Xaml.Controls.XamlControlsResources";
}
unsafe impl Send for XamlControlsResources {}
unsafe impl Sync for XamlControlsResources {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlControlsXamlMetaDataProvider(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlControlsXamlMetaDataProvider,
    windows_core::IUnknown,
    windows_core::IInspectable,
    IXamlMetadataProvider
);
impl XamlControlsXamlMetaDataProvider {
    pub fn new() -> windows_core::Result<Self> {
        Self::IActivationFactory(|f| f.ActivateInstance::<Self>())
    }
    fn IActivationFactory<
        R,
        F: FnOnce(&windows_core::imp::IGenericFactory) -> windows_core::Result<R>,
    >(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<
            XamlControlsXamlMetaDataProvider,
            windows_core::imp::IGenericFactory,
        > = windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlControlsXamlMetaDataProvider {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlMetadataProvider>();
}
unsafe impl windows_core::Interface for XamlControlsXamlMetaDataProvider {
    type Vtable = <IXamlMetadataProvider as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlMetadataProvider as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlControlsXamlMetaDataProvider {
    type Target = IXamlMetadataProvider;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlControlsXamlMetaDataProvider {
    const NAME: &'static str = "Microsoft.UI.Xaml.XamlTypeInfo.XamlControlsXamlMetaDataProvider";
}
unsafe impl Send for XamlControlsXamlMetaDataProvider {}
unsafe impl Sync for XamlControlsXamlMetaDataProvider {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlReader(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlReader,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl XamlReader {
    pub fn Load(xaml: &str) -> windows_core::Result<windows_core::IInspectable> {
        Self::IXamlReaderStatics(|this| unsafe {
            let mut result__ = core::mem::zeroed();
            (windows_core::Interface::vtable(this).Load)(
                windows_core::Interface::as_raw(this),
                core::mem::transmute_copy(&windows_core::HSTRING::from(xaml)),
                &mut result__,
            )
            .and_then(|| windows_core::imp::Type::from_abi(result__))
        })
    }
    fn IXamlReaderStatics<R, F: FnOnce(&IXamlReaderStatics) -> windows_core::Result<R>>(
        callback: F,
    ) -> windows_core::Result<R> {
        static SHARED: windows_core::imp::FactoryCache<XamlReader, IXamlReaderStatics> =
            windows_core::imp::FactoryCache::new();
        SHARED.call(callback)
    }
}
impl windows_core::RuntimeType for XamlReader {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlReader>();
}
unsafe impl windows_core::Interface for XamlReader {
    type Vtable = <IXamlReader as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlReader as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlReader {
    type Target = IXamlReader;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlReader {
    const NAME: &'static str = "Microsoft.UI.Xaml.Markup.XamlReader";
}
unsafe impl Send for XamlReader {}
unsafe impl Sync for XamlReader {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlRoot(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlRoot,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for XamlRoot {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlRoot>();
}
unsafe impl windows_core::Interface for XamlRoot {
    type Vtable = <IXamlRoot as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlRoot as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlRoot {
    type Target = IXamlRoot;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlRoot {
    const NAME: &'static str = "Microsoft.UI.Xaml.XamlRoot";
}
unsafe impl Send for XamlRoot {}
unsafe impl Sync for XamlRoot {}
#[repr(transparent)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XamlRootChangedEventArgs(windows_core::IUnknown);
windows_core::imp::interface_hierarchy!(
    XamlRootChangedEventArgs,
    windows_core::IUnknown,
    windows_core::IInspectable
);
impl windows_core::RuntimeType for XamlRootChangedEventArgs {
    const SIGNATURE: windows_core::imp::ConstBuffer =
        windows_core::imp::ConstBuffer::for_class::<Self, IXamlRootChangedEventArgs>();
}
unsafe impl windows_core::Interface for XamlRootChangedEventArgs {
    type Vtable = <IXamlRootChangedEventArgs as windows_core::Interface>::Vtable;
    const IID: windows_core::GUID = <IXamlRootChangedEventArgs as windows_core::Interface>::IID;
}
impl core::ops::Deref for XamlRootChangedEventArgs {
    type Target = IXamlRootChangedEventArgs;
    fn deref(&self) -> &Self::Target {
        unsafe { core::mem::transmute(self) }
    }
}
impl windows_core::RuntimeName for XamlRootChangedEventArgs {
    const NAME: &'static str = "Microsoft.UI.Xaml.XamlRootChangedEventArgs";
}
unsafe impl Send for XamlRootChangedEventArgs {}
unsafe impl Sync for XamlRootChangedEventArgs {}
#[repr(C)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct XmlnsDefinition {
    pub xml_namespace: windows_core::HSTRING,
    pub namespace: windows_core::HSTRING,
}
impl windows_core::imp::TypeKind for XmlnsDefinition {
    type TypeKind = windows_core::imp::CloneType;
}
impl windows_core::RuntimeType for XmlnsDefinition {
    const SIGNATURE: windows_core::imp::ConstBuffer = windows_core::imp::ConstBuffer::from_slice(
        b"struct(Microsoft.UI.Xaml.Markup.XmlnsDefinition;string;string)",
    );
}
