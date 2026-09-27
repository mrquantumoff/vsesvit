# Research: WinUI 3 + WebView2 from Rust (2026-09-27)

A research subagent built a pure-Rust unpackaged WinUI 3 prototype, and the orchestrator rebuilt and ran it against `tests/fixtures/extensions/probe` with DLLs taken from the official NuGet packages. Evidence: `evidence/winui-spike.png`.

## Crates

- Use `windows-bindgen = "0.100"` at build time and `windows-core`, `windows-future`, `windows-collections`, `windows-link`, `windows-reference` (all `0.100`) at run time.
- Do not add the `windows` crate. crates.io only has 0.62.2, which pulls windows-core 0.62 and mismatches types. Generate the few Win32 functions in the same bindgen run.
- `windows-reactor` 0.100 (Microsoft's declarative WinUI library) hard-codes the default WebView2 environment, so it cannot enable browser extensions. Its source (`crates/libs/reactor/src/native/winui/{app_shim.rs,bootstrap.rs}`) is a useful reference.

## Metadata (NuGet)

Microsoft.WindowsAppSDK 2.5.1 depends on WinUI 2.3.9, Foundation 2.3.12, InteractiveExperiences 2.1.9, Base 2.0.4. WinUI 2.3.9 requires Microsoft.Web.WebView2 >= 1.0.3719.77 (latest 1.0.4191.47).

| winmd | package path |
|---|---|
| `Microsoft.UI.Xaml.winmd`, `Microsoft.UI.Text.winmd` | `Microsoft.WindowsAppSDK.WinUI/2.3.9/metadata/` |
| `Microsoft.UI.winmd`, `Microsoft.Foundation.winmd`, `Microsoft.Graphics.winmd` | `Microsoft.WindowsAppSDK.InteractiveExperiences/2.1.9/metadata/10.0.18362.0/` |
| `Microsoft.Windows.*.winmd` | `Microsoft.WindowsAppSDK.Foundation/2.3.12/metadata/` |
| `Microsoft.Web.WebView2.Core.winmd` | `Microsoft.Web.WebView2/<ver>/lib/` |

## Bindgen (tested)

```rust
windows_bindgen::builder()
    .input("winmd").input_default().output("src/bindings.rs")
    .flat().minimal()
    .implements(["Microsoft.UI.Xaml.IApplicationOverrides", "Microsoft.UI.Xaml.Markup.IXamlMetadataProvider"])
    .compose("Microsoft.UI.Xaml.Application")
    .filter_file("bindings.txt")
    .write();
```

- Filters use raw ABI member names (`ITextBox::{get_Text, put_Text}`); an event name selects its add/remove pair; `Class::CreateInstance` yields `new()` (or `compose()` for the composed class).
- Win32 is one flat namespace in 0.100 (`Windows.Win32.CoInitializeEx`).
- Default (non-minimal) mode and namespace-wide filters did not compile or produced 44 MB of code.
- In minimal mode a class derefs only to its default interface; `cast::<IContentControl>()` etc. for the rest.

## Bootstrap (unpackaged)

- `MddBootstrapInitialize2(0x0002_0005, null, 0x0002_0005_0001_0000, 0)` with `Microsoft.WindowsAppRuntime.Bootstrap.dll` (Foundation `runtimes/win-x64/native/`) next to the exe, declared with `windows_link::link!`. COM must be STA-initialized first.
- Windows 11 alternative without the DLL: `TryCreatePackageDependency("Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe", 2.5.1.0, X64|Neutral)` + `AddPackageDependency` (what windows-reactor does). Also tested.
- No `resources.pri` or manifest needed. Call `SetProcessDpiAwarenessContext(PER_MONITOR_AWARE_V2)`.

## Files next to the exe

```
vsesvit.exe
Microsoft.WindowsAppRuntime.Bootstrap.dll   (only with MddBootstrap)
Microsoft.Web.WebView2.Core.dll             (Microsoft.Web.WebView2: runtimes/win-x64/native_uap/)
```

## App skeleton

`#[implement(IApplicationOverrides, IXamlMetadataProvider)] struct App`, delegate the three metadata methods to a lazily created `XamlControlsXamlMetaDataProvider`, merge `XamlControlsResources` in `OnLaunched`, build UI with `XamlReader::Load` and `FindName`, `Window::new`, `SetExtendsContentIntoTitleBar(true)`, `SetTitleBar`. Start with `Application::Start(ApplicationInitializationCallback::new(|_| Application::compose(App{..})))`.

## WebView2 with extensions

- `CoreWebView2EnvironmentOptions` via app-local `DllGetActivationFactory` (see pitfall 1), `ICoreWebView2EnvironmentOptions6::SetAreBrowserExtensionsEnabled(true)`, `ICoreWebView2EnvironmentStatics::CreateWithOptionsAsync("", user_data_folder, options)` called through the vtable (minimal mode gives statics no methods).
- Per tab: `WebView2::new()`, `IWebView22::EnsureCoreWebView2WithEnvironmentAsync(&env)`.
- Profile: `ICoreWebView2_13::Profile()`; add with `ICoreWebView2Profile7::AddBrowserExtensionAsync(dir)`; list with `CoreWebView2Profile_Manual3::GetBrowserExtensionsAsync()`; `CoreWebView2BrowserExtension` has `Id`, `Name`, `IsEnabled`, `EnableAsync`, `RemoveAsync`.
- Built-ins to hide from the list: Microsoft Clipboard Extension, Microsoft Edge PDF Viewer.
- Popups: navigating a WebView2 in the same profile to `chrome-extension://<id>/popup.html` works; `chrome.tabs.query`, `chrome.storage.sync`, `runtime.sendMessage` and `chrome.action` are available there.
- Limits: no browser UI entry points (the app draws action buttons and popups); unpacked folders only; names starting with `_` fail with E_ACCESSDENIED; editing files after install removes the extension; no mapping from extension tab ids to our WebView2s (WebView2Feedback #3853/#3854).

## Pitfalls

1. The 2.x framework manifest registers `Microsoft.Web.WebView2.Core.*` but does not ship the DLL, so activation fails with 0x8007007E. Load the app-local `Microsoft.Web.WebView2.Core.dll` with `LoadLibraryExW` + `DllGetActivationFactory`.
2. Merge `XamlControlsResources` in `OnLaunched` and implement `IXamlMetadataProvider` (microsoft-ui-xaml #7606).
3. An `Err` from `OnLaunched` kills the process with 0xC000027B; log first.
4. One shared `CoreWebView2Environment`; mismatched options across WebViews fail with ERROR_INVALID_STATE.
5. Always set a user data folder.
6. `windows-future` `.when()` closures must be `Send`; completions ran on the UI thread in testing.
7. `HSTRING` has no `Display` in 0.100 (`to_string_lossy`); boxing values goes through `windows-reference`.
8. Found by the orchestrator: a WebView2 set as `TabViewItem` content measured 1632x0 and was invisible. Hosting web views in a plain `Grid` below a strip-only `TabView` fixed it (1632x711). The shell uses that layout.
