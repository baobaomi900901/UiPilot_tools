#import "preflight.h"
#import "protocol.h"
#import <Security/Security.h>
BOOL probe_entitlements_valid(id value, BOOL service) {
    if (![value isKindOfClass:NSDictionary.class]) return NO;
    NSDictionary *d=value;
    if (!service) return d.count==0;
    id flag=d[@"com.apple.security.app-sandbox"];
    return d.count==1 && flag && CFGetTypeID((__bridge CFTypeRef)flag)==CFBooleanGetTypeID() &&
           (__bridge CFTypeRef)flag==kCFBooleanTrue;
}
static SecRequirementRef requirement(NSString *identifier) {
    SecRequirementRef result=NULL;
    NSString *rule=[NSString stringWithFormat:@"anchor apple generic and identifier \"%@\"",identifier];
    if (SecRequirementCreateWithString((__bridge CFStringRef)rule,kSecCSDefaultFlags,&result)!=errSecSuccess) return NULL;
    return result;
}
static NSDictionary *checkedInfo(SecStaticCodeRef code, BOOL service) {
    CFDictionaryRef info=NULL;
    if (SecCodeCopySigningInformation(code,kSecCSSigningInformation,&info)!=errSecSuccess || !info) {
        if (info) CFRelease(info); return nil;
    }
    NSDictionary *d=CFBridgingRelease(info);
    NSNumber *flags=d[(__bridge NSString *)kSecCodeInfoFlags];
    NSString *team=d[(__bridge NSString *)kSecCodeInfoTeamIdentifier];
    NSData *hash=d[(__bridge NSString *)kSecCodeInfoUnique];
    id entitlements=d[(__bridge NSString *)kSecCodeInfoEntitlementsDict] ?: @{};
    if (![flags isKindOfClass:NSNumber.class] || (flags.unsignedIntValue&kSecCodeSignatureAdhoc) ||
        ![team isKindOfClass:NSString.class] || team.length==0 ||
        ![hash isKindOfClass:NSData.class] || hash.length==0 || !probe_entitlements_valid(entitlements,service)) return nil;
    return d;
}
static NSDictionary *trusted(NSURL *url, NSString *identifier, BOOL service) {
    SecStaticCodeRef code=NULL;
    if (SecStaticCodeCreateWithPath((__bridge CFURLRef)url,kSecCSDefaultFlags,&code)!=errSecSuccess) return nil;
    SecRequirementRef rule=requirement(identifier);
    OSStatus status=rule?SecStaticCodeCheckValidity(code,kSecCSStrictValidate|kSecCSCheckAllArchitectures|kSecCSNoNetworkAccess,rule):errSecParam;
    // Never trust TeamIdentifier or entitlements before validating the signature.
    NSDictionary *info=status==errSecSuccess?checkedInfo(code,service):nil;
    if (rule) CFRelease(rule); CFRelease(code); return info;
}
static NSDictionary *runningIdentity(BOOL service) {
    SecCodeRef selfCode=NULL;
    if (SecCodeCopySelf(kSecCSDefaultFlags,&selfCode)!=errSecSuccess) return nil;
    SecRequirementRef rule=requirement(service?@PROBE_SERVICE_ID:@PROBE_HOST_ID);
    OSStatus status=rule?SecCodeCheckValidity(selfCode,kSecCSStrictValidate|kSecCSNoNetworkAccess,rule):errSecParam;
    // Public Security API accepts dynamic code and resolves its corresponding
    // static information. Dynamic validity is checked first, not inferred from
    // whichever app bundle currently occupies the executable pathname.
    NSDictionary *info=status==errSecSuccess?checkedInfo((SecStaticCodeRef)selfCode,service):nil;
    if (rule) CFRelease(rule); CFRelease(selfCode); return info;
}
BOOL probe_preflight(NSURL *host, BOOL runningService) {
    NSDictionary *running=runningIdentity(runningService);
    if (!running) return NO;
    NSURL *service=[host URLByAppendingPathComponent:@"Contents/XPCServices/BoundaryService.xpc" isDirectory:YES];
    NSDictionary *h=trusted(host,@PROBE_HOST_ID,NO);
    if (!h) return NO;
    NSDictionary *s=trusted(service,@PROBE_SERVICE_ID,YES);
    if (!s) return NO;
    NSString *teamKey=(__bridge NSString *)kSecCodeInfoTeamIdentifier;
    NSString *hashKey=(__bridge NSString *)kSecCodeInfoUnique;
    NSDictionary *onDisk=runningService?s:h;
    return [h[teamKey] isEqual:s[teamKey]] && [running[teamKey] isEqual:onDisk[teamKey]] &&
           [running[hashKey] isEqual:onDisk[hashKey]];
}
