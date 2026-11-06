#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <WebKit/WebKit.h>

// An independent, unprivileged window exercise. No Tauri IPC or application launches.
@interface ProbeDelegate : NSObject <NSApplicationDelegate, NSWindowDelegate, WKScriptMessageHandler>
@property(nonatomic, strong) NSWindow *window;
@property(nonatomic, strong) WKWebView *webView;
@property(nonatomic) EventHotKeyRef hotKey;
@property(nonatomic) EventHandlerRef keyHandler;
- (void)showWindow;
@end

static OSStatus handleHotKey(EventHandlerCallRef unused, EventRef event, void *context) {
    EventHotKeyID key = {0};
    if (GetEventParameter(event, kEventParamDirectObject, typeEventHotKeyID, NULL,
                          sizeof(key), NULL, &key) != noErr || key.signature != 'UiPk' || key.id != 1) {
        return eventNotHandledErr;
    }
    puts("global hotkey received");
    [(__bridge ProbeDelegate *)context showWindow];
    return noErr;
}

@implementation ProbeDelegate
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    self.window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 580, 275)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskMiniaturizable
        backing:NSBackingStoreBuffered defer:NO];
    self.window.title = @"UiPilot Native Focus Probe";
    self.window.releasedWhenClosed = NO;
    self.window.delegate = self;
    [self.window center];
    WKWebViewConfiguration *config = [[WKWebViewConfiguration alloc] init];
    [config.userContentController addScriptMessageHandler:self name:@"observation"];
    self.webView = [[WKWebView alloc] initWithFrame:self.window.contentView.bounds configuration:config];
    self.window.contentView = self.webView;
    NSString *page = @"<!doctype html><html><meta charset=utf-8><title>Native focus probe</title>"
        "<h2>快捷键与输入焦点测试</h2><p>先点一下终端，再按 ⌃⌥⇧U。松开按键，直接输入 abc，全程不要点击测试窗口。</p>"
        "<input autofocus id=q aria-label='Focus test'><p id=status></p>"
        "<script>function report(s){document.getElementById('status').textContent=s;window.webkit.messageHandlers.observation.postMessage(s)}"
        "const input=document.getElementById('q');input.oninput=()=>report('input observed (text omitted)');"
        "window.focusProbe=()=>{input.focus();report('input focused='+String(document.activeElement===input))};"
        "report('local page loaded');window.focusProbe()</script></html>";
    [self.webView loadHTMLString:page baseURL:nil];

    const EventTypeSpec spec = {kEventClassKeyboard, kEventHotKeyPressed};
    EventHandlerRef handler = NULL;
    OSStatus status = InstallApplicationEventHandler(handleHotKey, 1, &spec, (__bridge void *)self, &handler);
    self.keyHandler = handler;
    if (status == noErr) {
        const EventHotKeyID identifier = {'UiPk', 1};
        EventHotKeyRef keyRef = NULL;
        status = RegisterEventHotKey(kVK_ANSI_U, controlKey | optionKey | shiftKey, identifier,
                                     GetApplicationEventTarget(), 0, &keyRef);
        self.hotKey = keyRef;
    }
    printf("hotkey registration OSStatus=%d (0=success)\n", (int)status);
    if (status != noErr) self.window.title = @"UiPilot Native Probe — shortcut unavailable";
    [self showWindow];
}
- (void)showWindow {
    [NSApp activate];
    [self.window makeKeyAndOrderFront:self];
    [self.window makeFirstResponder:self.webView];
    [self.webView evaluateJavaScript:@"window.focusProbe && window.focusProbe()" completionHandler:nil];
    puts("show requested");
}
- (void)applicationDidBecomeActive:(NSNotification *)notification {
    puts("app active");
}
- (void)windowDidBecomeKey:(NSNotification *)notification {
    puts("window became key");
}
- (void)windowDidResignKey:(NSNotification *)notification {
    puts("window resigned key");
}
- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message {
    if (message.webView == self.webView && message.frameInfo.isMainFrame && [message.body isKindOfClass:NSString.class]) {
        printf("WKWebView: %s\n", [(NSString *)message.body UTF8String]);
    }
}
- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender { return YES; }
- (void)applicationWillTerminate:(NSNotification *)notification {
    if (self.hotKey) UnregisterEventHotKey(self.hotKey);
    if (self.keyHandler) RemoveEventHandler(self.keyHandler);
    [self.webView.configuration.userContentController removeScriptMessageHandlerForName:@"observation"];
    puts("native probe exited");
}
@end

int main(void) {
    @autoreleasepool {
        setbuf(stdout, NULL);
        NSApplication *app = NSApplication.sharedApplication;
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
        ProbeDelegate *delegate = [ProbeDelegate new];
        app.delegate = delegate;
        [app run];
    }
    return 0;
}
