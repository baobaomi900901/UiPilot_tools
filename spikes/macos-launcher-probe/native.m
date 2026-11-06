#import <Foundation/Foundation.h>
#import <WebKit/WebKit.h>

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2) {
            fprintf(stderr, "Usage: native-probe application-directory\n");
            return 2;
        }
        NSString *path = [NSString stringWithUTF8String:argv[1]];
        NSError *error = nil;
        NSArray *entries = [[NSFileManager defaultManager]
            contentsOfDirectoryAtURL:[NSURL fileURLWithPath:path isDirectory:YES]
            includingPropertiesForKeys:@[NSURLIsSymbolicLinkKey, NSURLIsDirectoryKey]
            options:NSDirectoryEnumerationSkipsHiddenFiles error:&error];
        if (!entries) {
            fprintf(stderr, "Cannot enumerate directory: %s\n", error.localizedDescription.UTF8String);
            return 1;
        }
        NSMutableArray *ids = [NSMutableArray array];
        for (NSURL *url in entries) {
            if (![url.pathExtension.lowercaseString isEqualToString:@"app"]) continue;
            NSDictionary *properties = [url resourceValuesForKeys:@[NSURLIsSymbolicLinkKey, NSURLIsDirectoryKey] error:nil];
            if (![properties[NSURLIsDirectoryKey] boolValue] || [properties[NSURLIsSymbolicLinkKey] boolValue]) continue;
            NSString *identifier = [NSBundle bundleWithURL:url].bundleIdentifier;
            if (identifier.length) [ids addObject:identifier];
        }
        [ids sortUsingSelector:@selector(compare:)];
        // Referencing the linked class does not create an NSApplication or WebView.
        BOOL webkit = [WKWebView class] != Nil;
        NSDictionary *report = @{@"wkWebViewClassLinked": @(webkit), @"bundleIDs": ids};
        NSData *json = [NSJSONSerialization dataWithJSONObject:report options:0 error:&error];
        if (!json || !webkit) return 1;
        fwrite(json.bytes, 1, json.length, stdout);
        putchar('\n');
        return 0;
    }
}
