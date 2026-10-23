use std::sync::{mpsc, Arc};
use webview2_com::{
    ContextMenuRequestedEventHandler,
    Microsoft::Web::WebView2::Win32::{
        ICoreWebView2ContextMenuRequestedEventArgs, ICoreWebView2Settings3, ICoreWebView2_11,
    },
};
use windows::core::{Interface, BOOL};

use crate::{diagnostics::Diagnostics, navigation::keep_default_context_menu};

pub fn apply(
    window: &tauri::WebviewWindow,
    diagnostics: Arc<Diagnostics>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (sender, receiver) = mpsc::channel();
    // Setup calls this on the main thread, where with_webview runs synchronously.
    window.with_webview(move |webview| {
        let result = configure(webview, diagnostics);
        let _ = sender.send(result);
    })?;
    receiver.recv()??;
    Ok(())
}

fn configure(
    webview: tauri::webview::PlatformWebview,
    diagnostics: Arc<Diagnostics>,
) -> windows::core::Result<()> {
    // The controller and all COM calls stay on WebView2's owning UI thread.
    unsafe {
        let core = webview.controller().CoreWebView2()?;
        let settings = core.Settings()?.cast::<ICoreWebView2Settings3>()?;
        settings.SetAreBrowserAcceleratorKeysEnabled(false)?;
        let core = core.cast::<ICoreWebView2_11>()?;
        let handler = ContextMenuRequestedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                if let Err(error) = filter_context_menu(&args) {
                    diagnostics.log(&format!("failed to apply context menu policy: {error}"));
                }
            }
            Ok(())
        }));
        let mut token = 0;
        // WebView2 retains the handler until this webview is destroyed.
        core.add_ContextMenuRequested(&handler, &mut token)?;
    }
    Ok(())
}

fn filter_context_menu(
    args: &ICoreWebView2ContextMenuRequestedEventArgs,
) -> windows::core::Result<()> {
    unsafe {
        // Suppress the menu unless both target queries succeed and policy allows it.
        args.SetHandled(true)?;
        let target = args.ContextMenuTarget()?;
        let mut editable = BOOL::default();
        let mut selection = BOOL::default();
        target.IsEditable(&mut editable)?;
        target.HasSelection(&mut selection)?;
        if keep_default_context_menu(editable.as_bool(), selection.as_bool()) {
            args.SetHandled(false)?;
        }
    }
    Ok(())
}
