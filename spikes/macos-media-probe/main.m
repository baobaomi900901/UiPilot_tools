#import <AppKit/AppKit.h>
#import <WebKit/WebKit.h>
#import <math.h>
#import "lifecycle.h"

@interface ProbeApp : NSObject <NSApplicationDelegate, NSWindowDelegate, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler, WKURLSchemeHandler> {
    ProbeLifecycle _life;
}
@property NSWindow *window;
@property NSTextView *logView;
@property NSButton *speech;
@property WKWebView *webView;
@property WKNavigation *expectedNavigation;
@property NSURL *fixtureURL;
@property NSString *fixtureHTML;
@property NSString *nonce;
@property BOOL navigationPermit;
@property BOOL suspendedCase;
@property BOOL speechCase;
@property BOOL finished;
@property BOOL fixtureStarted;
@property BOOL progressed;
@property BOOL htmlProgressed;
@property BOOL audioProgressed;
@property BOOL speechStarted;
@property NSInteger sequenceStep;
@property BOOL sequence;
@property NSUInteger runSerial;
@end

@implementation ProbeApp
- (void)log:(NSString *)text {
    NSString *line=[text stringByAppendingString:@"\n"];
    if(self.logView.string.length>200000)self.logView.string=@"（较早日志已清理）\n";
    [self.logView.textStorage appendAttributedString:[[NSAttributedString alloc] initWithString:line attributes:@{NSForegroundColorAttributeName:NSColor.textColor,NSFontAttributeName:[NSFont monospacedSystemFontOfSize:11 weight:NSFontWeightRegular]}]];
    [self.logView scrollRangeToVisible:NSMakeRange(self.logView.string.length,0)];
}
- (NSButton *)button:(NSString *)title action:(SEL)action key:(NSString *)key {
    NSButton *b=[NSButton buttonWithTitle:title target:self action:action];
    b.keyEquivalent=key;b.keyEquivalentModifierMask=NSEventModifierFlagCommand;
    b.accessibilityLabel=title;return b;
}
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    (void)notification;
    self.window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,880,620) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
    self.window.title=@"WKWebView 媒体暂停探针 · 非生产保障";self.window.delegate=self;self.window.minSize=NSMakeSize(800,560);
    NSTextField *intro=[NSTextField wrappingLabelWithString:@"默认不创建媒体页面。基线会尝试低音量短音（原生 2 秒截止）；暂停案例也可能发声。结果仅为本次观测，不证明全部音频隔离，插件仍关闭。原生状态 None / Paused 不代表静音开关。"];
    self.speech=[NSButton checkboxWithTitle:@"额外测试语音合成（可能听到“测试”，默认关闭）" target:nil action:nil];
    NSStackView *buttons=[NSStackView stackViewWithViews:@[
        [self button:@"播放短音基线" action:@selector(baseline:) key:@"1"],
        [self button:@"先暂停再加载" action:@selector(suspended:) key:@"2"],
        [self button:@"暂停 → reload → 新导航" action:@selector(navigationSequence:) key:@"3"],
        [self button:@"迟到回调 / 替换实例" action:@selector(late:) key:@"4"],
        [self button:@"取消" action:@selector(cancel:) key:@"."]]];
    buttons.orientation=NSUserInterfaceLayoutOrientationHorizontal;buttons.spacing=8;
    NSTextField *hint=[NSTextField wrappingLabelWithString:@"顺序：基线 → 暂停 → 导航 → 迟到。⌘1–4 启动，⌘. 取消。日志可选择后手动 ⌘C；不自动访问剪贴板。WebContent 进程更换始终另记未验证。"];
    NSScrollView *scroll=[[NSScrollView alloc] initWithFrame:NSZeroRect];scroll.hasVerticalScroller=YES;scroll.borderType=NSBezelBorder;
    self.logView=[[NSTextView alloc] initWithFrame:NSMakeRect(0,0,840,420)];self.logView.editable=NO;self.logView.selectable=YES;self.logView.autoresizingMask=NSViewWidthSizable;self.logView.textContainer.widthTracksTextView=YES;scroll.documentView=self.logView;
    NSStackView *layout=[NSStackView stackViewWithViews:@[intro,self.speech,buttons,hint,scroll]];
    layout.orientation=NSUserInterfaceLayoutOrientationVertical;layout.alignment=NSLayoutAttributeLeading;layout.spacing=12;layout.translatesAutoresizingMaskIntoConstraints=NO;
    [self.window.contentView addSubview:layout];
    [NSLayoutConstraint activateConstraints:@[[layout.leadingAnchor constraintEqualToAnchor:self.window.contentView.leadingAnchor constant:16],[layout.trailingAnchor constraintEqualToAnchor:self.window.contentView.trailingAnchor constant:-16],[layout.topAnchor constraintEqualToAnchor:self.window.contentView.topAnchor constant:16],[layout.bottomAnchor constraintEqualToAnchor:self.window.contentView.bottomAnchor constant:-16],[scroll.widthAnchor constraintEqualToAnchor:layout.widthAnchor],[scroll.heightAnchor constraintGreaterThanOrEqualToConstant:300],[intro.widthAnchor constraintEqualToAnchor:layout.widthAnchor],[hint.widthAnchor constraintEqualToAnchor:layout.widthAnchor]]];
    NSMenu *menu=[NSMenu new];NSMenuItem *application=[NSMenuItem new];[menu addItem:application];NSMenu *app=[NSMenu new];[app addItemWithTitle:@"退出探针" action:@selector(terminate:) keyEquivalent:@"q"];application.submenu=app;
    NSMenuItem *edit=[NSMenuItem new];[menu addItem:edit];NSMenu *editMenu=[[NSMenu alloc] initWithTitle:@"编辑"];[editMenu addItemWithTitle:@"复制所选日志" action:@selector(copy:) keyEquivalent:@"c"];[editMenu addItemWithTitle:@"全选" action:@selector(selectAll:) keyEquivalent:@"a"];edit.submenu=editMenu;NSApp.mainMenu=menu;
    [self log:@"就绪：尚未创建 WKWebView；语音、HTMLMedia、WebAudio 均未验证。"];
    [self.window center];[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];
}
- (BOOL)owns:(WKWebView *)view ticket:(ProbeTicket)ticket {
    return view && view==self.webView && probe_current(&_life,ticket);
}
- (void)stop:(NSString *)reason {
    // Retirement MUST precede every native cleanup call and delayed completion.
    probe_retire(&_life);self.runSerial++;self.navigationPermit=NO;
    WKWebView *view=self.webView;self.webView=nil;self.expectedNavigation=nil;
    if(view){
        @try { [view stopLoading];[view evaluateJavaScript:@"if(window.probeStop)window.probeStop();" completionHandler:nil];
            [view setAllMediaPlaybackSuspended:YES completionHandler:nil];
        } @catch(NSException *exception){(void)exception;[self log:@"清理：原生异常（不确定）"];} 
        [view.configuration.userContentController removeScriptMessageHandlerForName:@"probe"];
        view.navigationDelegate=nil;view.UIDelegate=nil;[view removeFromSuperview];
    }
    self.fixtureHTML=nil;self.fixtureURL=nil;self.nonce=nil;
    if(reason)[self log:reason];
}
- (void)cancel:(id)sender {(void)sender;[self stop:@"已退役当前案例并请求停止/销毁；迟到回调无权加载。"];} 
- (void)windowWillClose:(NSNotification *)notification {(void)notification;[self stop:nil];[NSApp terminate:nil];}
- (void)applicationWillTerminate:(NSNotification *)notification {(void)notification;[self stop:nil];}
- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender {(void)sender;return YES;}
- (WKWebView *)createView {
    WKWebViewConfiguration *configuration=[WKWebViewConfiguration new];
    configuration.websiteDataStore=WKWebsiteDataStore.nonPersistentDataStore;
    configuration.mediaTypesRequiringUserActionForPlayback=WKAudiovisualMediaTypeNone;
    configuration.allowsAirPlayForMediaPlayback=NO;
    [configuration.userContentController addScriptMessageHandler:self name:@"probe"];
    [configuration setURLSchemeHandler:self forURLScheme:@"uipilot-media-probe"];
    WKWebView *view=[[WKWebView alloc] initWithFrame:NSMakeRect(-10,-10,1,1) configuration:configuration];
    view.navigationDelegate=self;view.UIDelegate=self;view.hidden=YES;
    [self.window.contentView addSubview:view];self.webView=view;
    // No load, initialization script or package code precedes suspension.
    return view;
}
- (void)baseline:(id)sender {(void)sender;[self start:NO sequence:NO delay:NO];}
- (void)suspended:(id)sender {(void)sender;[self start:YES sequence:NO delay:NO];}
- (void)navigationSequence:(id)sender {(void)sender;[self start:YES sequence:YES delay:NO];}
- (void)late:(id)sender {(void)sender;[self start:YES sequence:NO delay:YES];}
- (void)start:(BOOL)suspend sequence:(BOOL)sequence delay:(BOOL)delay {
    [self stop:nil];self.sequence=sequence;self.sequenceStep=0;self.suspendedCase=suspend;self.speechCase=self.speech.state==NSControlStateValueOn;
    @try {
        WKWebView *view=[self createView];ProbeTicket ticket=probe_begin(&_life,(uintptr_t)(__bridge void *)view,suspend);
        if(!ticket.generation){[self stop:@"案例拒绝：代际耗尽。"] ;return;}
        [self log:[NSString stringWithFormat:@"开始 %@，generation=%llu，语音=%@。",suspend?@"先暂停":@"短音基线",(unsigned long long)ticket.generation,self.speechCase?@"选择测试":@"未验证"]];
        if(!suspend){[self load:view ticket:ticket reload:NO];return;}
        __weak ProbeApp *weakSelf=self;__weak WKWebView *weakView=view;
        // A preparation timeout retires first. It never loads on timeout/error.
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,3*NSEC_PER_SEC),dispatch_get_main_queue(),^{
            ProbeApp *s=weakSelf;if(s && [s owns:weakView ticket:ticket] && s->_life.phase==PROBE_WAITING)[s stop:@"暂停 completion 超时：不加载，结果不确定。"];
        });
        [view setAllMediaPlaybackSuspended:YES completionHandler:^{
            dispatch_async(dispatch_get_main_queue(),^{
                ProbeApp *s=weakSelf;if(!s)return;
                if(delay){
                    if(![s owns:weakView ticket:ticket])return;
                    [s log:@"旧暂停 completion 已到；延迟消费，先替换实例。"];
                    [s start:YES sequence:NO delay:NO];
                    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,500*NSEC_PER_MSEC),dispatch_get_main_queue(),^{
                        ProbeApp *owner=weakSelf;if(!owner)return;
                        [owner completeSuspension:weakView ticket:ticket]; 
                    });
                    return;
                }
                [s completeSuspension:weakView ticket:ticket];
            });
        }];
    } @catch(NSException *exception){(void)exception;[self stop:@"准备原生异常：不再加载，结果不确定。"];} 
}
- (void)completeSuspension:(WKWebView *)view ticket:(ProbeTicket)ticket {
    if(![self owns:view ticket:ticket] || !probe_suspend_completed(&_life,ticket)) {
        [self log:@"迟到/重复 completion 已拒绝；未加载旧 fixture、未退役新实例。"];
        return;
    }
    [self log:@"暂停 completion 到达（不是 mute proof）；即将准入固定 fixture。"];
    [self readNative:view ticket:ticket stage:@"暂停后、加载前"];
    [self load:view ticket:ticket reload:NO];
}
- (void)load:(WKWebView *)view ticket:(ProbeTicket)ticket reload:(BOOL)reload {
    if(![self owns:view ticket:ticket] || !probe_admit_load(&_life,ticket))return;
    self.finished=NO;self.fixtureStarted=NO;self.progressed=NO;self.htmlProgressed=NO;self.audioProgressed=NO;self.speechStarted=NO;
    if(!reload){self.nonce=NSUUID.UUID.UUIDString;self.fixtureURL=[NSURL URLWithString:[@"uipilot-media-probe://fixture/" stringByAppendingString:self.nonce]];}
    NSString *path=[NSBundle.mainBundle pathForResource:@"fixture" ofType:@"html"];
    NSString *html=path?[NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:NULL]:nil;
    if(!html || html.length>20000){[self stop:@"固定 fixture 缺失或过大：不加载。"] ;return;}
    self.fixtureHTML=[[[html stringByReplacingOccurrencesOfString:@"__NONCE__" withString:self.nonce] stringByReplacingOccurrencesOfString:@"__GENERATION__" withString:[NSString stringWithFormat:@"%llu",(unsigned long long)ticket.generation]] stringByReplacingOccurrencesOfString:@"__SPEECH__" withString:self.speechCase?@"true":@"false"];
    self.navigationPermit=YES;
    @try {self.expectedNavigation=reload?[view reloadFromOrigin]:[view loadRequest:[NSURLRequest requestWithURL:self.fixtureURL cachePolicy:NSURLRequestReloadIgnoringLocalCacheData timeoutInterval:2]];}
    @catch(NSException *exception){(void)exception;[self stop:@"导航原生异常：已退役，不确定。"] ;return;}
    if(!self.expectedNavigation){[self stop:@"原生未返回导航：不确定。"] ;return;}
    [self log:[NSString stringWithFormat:@"准入 %@，generation=%llu，阶段=%ld。",reload?@"reload":@"固定新导航",(unsigned long long)ticket.generation,(long)self.sequenceStep]];
    __weak ProbeApp *weakSelf=self;__weak WKWebView *weakView=view;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,NSEC_PER_SEC),dispatch_get_main_queue(),^{
        ProbeApp *s=weakSelf;if(s && [s owns:weakView ticket:ticket])[s readNative:weakView ticket:ticket stage:@"加载准入后 1 秒"];
    });
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,2*NSEC_PER_SEC),dispatch_get_main_queue(),^{
        ProbeApp *s=weakSelf;WKWebView *v=weakView;if(!s || ![s owns:v ticket:ticket])return;
        BOOL next=s.sequence && s.sequenceStep<2 && s.finished;
        NSString *outcome=(!s.fixtureStarted || !s.finished)?@"缺报告/超时：不确定":(s.suspendedCase?(s.progressed?@"可疑播放推进":@"本次未观察到推进；不等于全部音频隔离"):(s.progressed?@"基线观察到媒体时钟推进":@"基线未推进：不确定"));
        [s log:outcome];
        [s log:[NSString stringWithFormat:@"逐通道：HTML 时间推进=%@；WebAudio 时间推进=%@。没有推进的基线通道未验证。",s.htmlProgressed?@"有":@"未观察到",s.audioProgressed?@"有":@"未观察到"]];
        if(s.speechCase)[s log:s.speechStarted?@"语音观察到 start；是否受暂停覆盖需单列判断。":@"语音未观察到 start，仍未验证。"];
        // Retire before cleanup; native suspension is independent of JS timers.
        probe_retire(&s->_life);s.navigationPermit=NO;
        @try {if(!next)[v evaluateJavaScript:@"if(window.probeStop)window.probeStop();" completionHandler:nil];[v stopLoading];}
        @catch(NSException *exception){(void)exception;[s stop:@"截止清理异常：不确定。"] ;return;}
        if(next){
            s.sequenceStep++;ProbeTicket newTicket=probe_begin(&s->_life,(uintptr_t)(__bridge void *)v,NO);
            [s load:v ticket:newTicket reload:s.sequenceStep==1];
        }else{[s stop:@"原生截止：请求暂停并销毁。WebContent 进程更换未强制触发，未验证。"];} 
    });
}
- (void)readNative:(WKWebView *)view ticket:(ProbeTicket)ticket stage:(NSString *)stage {
    if(![self owns:view ticket:ticket])return;
    __weak ProbeApp *weakSelf=self;__weak WKWebView *weakView=view;
    @try {[view requestMediaPlaybackStateWithCompletionHandler:^(WKMediaPlaybackState state){
        dispatch_async(dispatch_get_main_queue(),^{ProbeApp *s=weakSelf;if(!s || ![s owns:weakView ticket:ticket])return;
            NSString *raw=@"Unknown";switch(state){case WKMediaPlaybackStateNone:raw=@"None";break;case WKMediaPlaybackStatePlaying:raw=@"Playing";break;case WKMediaPlaybackStatePaused:raw=@"Paused";break;case WKMediaPlaybackStateSuspended:raw=@"Suspended";break;}
            [s log:[NSString stringWithFormat:@"原生 %@：%@（活动状态，非开关读回）",stage,raw]];
        });
    }];} @catch(NSException *exception){(void)exception;[self stop:@"读取原生状态异常：不确定。"];} 
}
- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message {
    (void)controller;NSDictionary *body=[message.body isKindOfClass:NSDictionary.class]?message.body:nil;
    if(!body || body.count!=7)return;
    NSString *gen=body[@"generation"],*nonce=body[@"nonce"],*kind=body[@"kind"],*state=body[@"audioState"];
    if(![gen isKindOfClass:NSString.class] || ![nonce isKindOfClass:NSString.class] || ![kind isKindOfClass:NSString.class] || ![state isKindOfClass:NSString.class])return;
    ProbeTicket ticket=_life.current;
    if(message.webView!=self.webView || ![gen isEqualToString:[NSString stringWithFormat:@"%llu",(unsigned long long)ticket.generation]] || !probe_accept_message(&_life,ticket,message.frameInfo.mainFrame,[nonce isEqualToString:self.nonce]))return;
    NSSet *kinds=[NSSet setWithArray:@[@"fixtureStarted",@"sample",@"htmlStarted",@"htmlRejected",@"htmlError",@"audioResumed",@"audioRejected",@"audioError",@"speechStart",@"speechEnd",@"speechError",@"finished"]];
    NSSet *states=[NSSet setWithArray:@[@"unavailable",@"suspended",@"running",@"closed",@"interrupted"]];
    NSNumber *h=body[@"htmlTime"],*a=body[@"audioTime"],*paused=body[@"htmlPaused"];
    if(![kinds containsObject:kind] || ![states containsObject:state] || ![h isKindOfClass:NSNumber.class] || ![a isKindOfClass:NSNumber.class] || ![paused isKindOfClass:NSNumber.class] || !isfinite(h.doubleValue) || !isfinite(a.doubleValue) || h.doubleValue<0 || a.doubleValue<0 || h.doubleValue>10 || a.doubleValue>10)return;
    if([kind isEqualToString:@"fixtureStarted"])self.fixtureStarted=YES;
    if([kind isEqualToString:@"finished"])self.finished=YES;
    if(h.doubleValue>0.05)self.htmlProgressed=YES;
    if(a.doubleValue>0.05)self.audioProgressed=YES;
    if(self.htmlProgressed || self.audioProgressed)self.progressed=YES;
    if([kind isEqualToString:@"speechStart"]){self.speechStarted=YES;if(self.suspendedCase)self.progressed=YES;}
    [self log:[NSString stringWithFormat:@"g=%llu %@：HTML paused=%d t=%.3f，WebAudio %@ t=%.3f",(unsigned long long)ticket.generation,kind,paused.boolValue,h.doubleValue,state,a.doubleValue]];
}
- (void)webView:(WKWebView *)view startURLSchemeTask:(id<WKURLSchemeTask>)task {
    if(view!=self.webView || _life.phase!=PROBE_LOADED || ![task.request.URL isEqual:self.fixtureURL] || !self.fixtureHTML){[task didFailWithError:[NSError errorWithDomain:@"ProbeDenied" code:1 userInfo:nil]];return;}
    NSData *data=[self.fixtureHTML dataUsingEncoding:NSUTF8StringEncoding];
    NSHTTPURLResponse *response=[[NSHTTPURLResponse alloc] initWithURL:self.fixtureURL statusCode:200 HTTPVersion:@"HTTP/1.1" headerFields:@{@"Content-Type":@"text/html; charset=utf-8",@"Cache-Control":@"no-store"}];
    [task didReceiveResponse:response];[task didReceiveData:data];[task didFinish];
}
- (void)webView:(WKWebView *)view stopURLSchemeTask:(id<WKURLSchemeTask>)task {(void)view;(void)task;}
- (void)webView:(WKWebView *)view decidePolicyForNavigationAction:(WKNavigationAction *)action decisionHandler:(void (^)(WKNavigationActionPolicy))handler {
    BOOL allow=view==self.webView && self.navigationPermit && action.targetFrame.mainFrame && !action.shouldPerformDownload && [action.request.URL isEqual:self.fixtureURL] && _life.phase==PROBE_LOADED;
    if(allow)self.navigationPermit=NO;handler(allow?WKNavigationActionPolicyAllow:WKNavigationActionPolicyCancel);
}
- (void)webView:(WKWebView *)view decidePolicyForNavigationResponse:(WKNavigationResponse *)response decisionHandler:(void (^)(WKNavigationResponsePolicy))handler {
    BOOL allow=view==self.webView && _life.phase==PROBE_LOADED && response.forMainFrame && response.canShowMIMEType && [response.response.URL isEqual:self.fixtureURL] && [response.response.MIMEType isEqualToString:@"text/html"];
    handler(allow?WKNavigationResponsePolicyAllow:WKNavigationResponsePolicyCancel);
}
- (void)webView:(WKWebView *)view requestMediaCapturePermissionForOrigin:(WKSecurityOrigin *)origin initiatedByFrame:(WKFrameInfo *)frame type:(WKMediaCaptureType)type decisionHandler:(void (^)(WKPermissionDecision))handler {(void)view;(void)origin;(void)frame;(void)type;handler(WKPermissionDecisionDeny);}
- (WKWebView *)webView:(WKWebView *)view createWebViewWithConfiguration:(WKWebViewConfiguration *)configuration forNavigationAction:(WKNavigationAction *)action windowFeatures:(WKWindowFeatures *)features {(void)view;(void)configuration;(void)action;(void)features;return nil;}
- (void)webView:(WKWebView *)view didFinishNavigation:(WKNavigation *)navigation {if(view==self.webView && navigation==self.expectedNavigation)[self readNative:view ticket:_life.current stage:@"固定页面加载后"];} 
- (void)webView:(WKWebView *)view didFailNavigation:(WKNavigation *)navigation withError:(NSError *)error {(void)error;if(view==self.webView && navigation==self.expectedNavigation)[self stop:@"导航失败：不确定。"];} 
- (void)webView:(WKWebView *)view didFailProvisionalNavigation:(WKNavigation *)navigation withError:(NSError *)error {[self webView:view didFailNavigation:navigation withError:error];}
- (void)webViewWebContentProcessDidTerminate:(WKWebView *)view {if(view==self.webView)[self stop:@"WebContent 进程终止：已退役；恢复未验证。"];} 
@end
int main(void) {@autoreleasepool {NSApplication *application=NSApplication.sharedApplication;[application setActivationPolicy:NSApplicationActivationPolicyRegular];ProbeApp *delegate=[ProbeApp new];application.delegate=delegate;[application run];}return 0;}
