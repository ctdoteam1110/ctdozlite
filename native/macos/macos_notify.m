#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <UserNotifications/UserNotifications.h>
#import <objc/runtime.h>

static BOOL ctdozlite_has_app_bundle(void) {
    NSBundle *main = [NSBundle mainBundle];
    NSString *bid = main.bundleIdentifier;
    if (bid == nil || bid.length == 0) {
        return NO;
    }
    NSString *ext = main.bundleURL.pathExtension;
    return ext != nil && [ext caseInsensitiveCompare:@"app"] == NSOrderedSame;
}

@interface CtdozNotificationDelegate : NSObject <UNUserNotificationCenterDelegate>
@end

@implementation CtdozNotificationDelegate

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
       willPresentNotification:(UNNotification *)notification
         withCompletionHandler:(void (^)(UNNotificationPresentationOptions options))completionHandler {
    (void)center;
    UNNotificationPresentationOptions opts =
        UNNotificationPresentationOptionBanner | UNNotificationPresentationOptionList;
    if (notification.request.content.sound != nil) {
        opts |= UNNotificationPresentationOptionSound;
    }
    completionHandler(opts);
}

- (void)userNotificationCenter:(UNUserNotificationCenter *)center
didReceiveNotificationResponse:(UNNotificationResponse *)response
         withCompletionHandler:(void (^)(void))completionHandler {
    (void)center;
    (void)response;
    dispatch_async(dispatch_get_main_queue(), ^{
        NSApplication *app = [NSApplication sharedApplication];
        [app unhide:nil];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        [app activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
        for (NSWindow *win in [app windows]) {
            if ([win isMiniaturized]) {
                [win deminiaturize:nil];
            }
            if ([win canBecomeKeyWindow]) {
                [win makeKeyAndOrderFront:nil];
            }
        }
    });
    completionHandler();
}

@end

static CtdozNotificationDelegate *g_delegate = nil;

static UNUserNotificationCenter *ctdozlite_ensure_center(void) {
    if (!ctdozlite_has_app_bundle()) {
        return nil;
    }
    @try {
        UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
        static dispatch_once_t onceToken;
        dispatch_once(&onceToken, ^{
            g_delegate = [[CtdozNotificationDelegate alloc] init];
            center.delegate = g_delegate;
        });
        return center;
    } @catch (NSException *ex) {
        (void)ex;
        return nil;
    }
}

void ctdozlite_macos_notify_setup(int request_if_undetermined) {
    @autoreleasepool {
        UNUserNotificationCenter *center = ctdozlite_ensure_center();
        if (center == nil || !request_if_undetermined) {
            return;
        }
        [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            if (settings.authorizationStatus == UNAuthorizationStatusNotDetermined) {
                UNAuthorizationOptions opts =
                    UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
                [center requestAuthorizationWithOptions:opts completionHandler:^(BOOL granted, NSError *error) {
                    (void)granted;
                    (void)error;
                }];
            }
        }];
    }
}

static void ctdozlite_post_request(UNUserNotificationCenter *center,
                                   NSString *title,
                                   NSString *body,
                                   NSString *soundName) {
    UNMutableNotificationContent *content = [[UNMutableNotificationContent alloc] init];
    content.title = title ?: @"";
    content.body = body ?: @"";
    if (soundName != nil && soundName.length > 0) {
        content.sound = [UNNotificationSound defaultSound];
    }
    NSString *ident = [[NSUUID UUID] UUIDString];
    UNNotificationRequest *req =
        [UNNotificationRequest requestWithIdentifier:ident content:content trigger:nil];
    [center addNotificationRequest:req withCompletionHandler:^(NSError *error) {
        (void)error;
    }];
}

int ctdozlite_macos_notify(const char *title_utf8,
                           const char *body_utf8,
                           const char *sound_utf8) {
    @autoreleasepool {
        UNUserNotificationCenter *center = ctdozlite_ensure_center();
        if (center == nil) {
            return 0;
        }
        NSString *title = title_utf8 ? [NSString stringWithUTF8String:title_utf8] : @"";
        NSString *body = body_utf8 ? [NSString stringWithUTF8String:body_utf8] : @"";
        NSString *sound = (sound_utf8 && sound_utf8[0] != '\0')
                              ? [NSString stringWithUTF8String:sound_utf8]
                              : nil;
        [center getNotificationSettingsWithCompletionHandler:^(UNNotificationSettings *settings) {
            UNAuthorizationStatus status = settings.authorizationStatus;
            if (status == UNAuthorizationStatusAuthorized ||
                status == UNAuthorizationStatusProvisional) {
                ctdozlite_post_request(center, title, body, sound);
            } else if (status == UNAuthorizationStatusNotDetermined) {
                UNAuthorizationOptions opts =
                    UNAuthorizationOptionAlert | UNAuthorizationOptionSound | UNAuthorizationOptionBadge;
                [center requestAuthorizationWithOptions:opts completionHandler:^(BOOL granted, NSError *error) {
                    (void)error;
                    if (granted) {
                        ctdozlite_post_request(center, title, body, sound);
                    }
                }];
            }
        }];
        return 1;
    }
}

static BOOL ctdozlite_reopen_handler(id self, SEL _cmd, NSApplication *sender, BOOL flag) {
    (void)self;
    (void)_cmd;
    (void)flag;
    for (NSWindow *win in [sender windows]) {
        NSString *title = [win title];
        if (title != nil &&
            ([title isEqualToString:@"ctdozlite"] ||
             ([title hasPrefix:@"("] && [title hasSuffix:@") ctdozlite"]))) {
            if ([win isMiniaturized]) {
                [win deminiaturize:nil];
            }
            [win makeKeyAndOrderFront:nil];
            break;
        }
    }
    [sender unhide:nil];
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    [sender activateIgnoringOtherApps:YES];
#pragma clang diagnostic pop
    return YES;
}

void ctdozlite_macos_install_reopen_handler(void) {
    @autoreleasepool {
        static dispatch_once_t onceToken;
        dispatch_once(&onceToken, ^{
            id delegate = [[NSApplication sharedApplication] delegate];
            if (delegate != nil) {
                Class cls = object_getClass(delegate);
                SEL sel = @selector(applicationShouldHandleReopen:hasVisibleWindows:);
                if (!class_respondsToSelector(cls, sel)) {
                    NSString *enc = [NSString stringWithFormat:@"%s@:@%s", @encode(BOOL), @encode(BOOL)];
                    class_addMethod(cls, sel, (IMP)ctdozlite_reopen_handler, [enc UTF8String]);
                }
            }
        });
    }
}
