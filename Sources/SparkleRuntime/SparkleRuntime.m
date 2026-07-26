#import "SparkleRuntime.h"

#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

@interface FanControlSparkleDelegate : NSObject
@end

@implementation FanControlSparkleDelegate

- (BOOL)supportsGentleScheduledUpdateReminders {
    return YES;
}

- (void)standardUserDriverWillHandleShowingUpdate:(BOOL)handleShowingUpdate
                                        forUpdate:(id)update
                                            state:(id)state {
    (void)handleShowingUpdate;
    (void)update;
    (void)state;
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
}

- (void)standardUserDriverWillFinishUpdateSession {
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
}

@end

static id updaterController;
static id updater;
static FanControlSparkleDelegate *userDriverDelegate;

static id invokeObjectGetter(id target, SEL selector) {
    if (target == nil || ![target respondsToSelector:selector]) {
        return nil;
    }

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Warc-performSelector-leaks"
    id result = [target performSelector:selector];
#pragma clang diagnostic pop
    return result;
}

static BOOL invokeBooleanGetter(id target, SEL selector) {
    if (target == nil || ![target respondsToSelector:selector]) {
        return NO;
    }

    NSMethodSignature *signature = [target methodSignatureForSelector:selector];
    NSInvocation *invocation = [NSInvocation invocationWithMethodSignature:signature];
    invocation.selector = selector;
    invocation.target = target;
    [invocation invoke];

    BOOL result = NO;
    [invocation getReturnValue:&result];
    return result;
}

bool FanControlInitializeUpdater(void) {
    @autoreleasepool {
        if (updaterController != nil) {
            return true;
        }

        NSURL *frameworksURL = NSBundle.mainBundle.privateFrameworksURL;
        NSURL *sparkleURL = [frameworksURL URLByAppendingPathComponent:@"Sparkle.framework"];
        NSBundle *sparkleBundle = [NSBundle bundleWithURL:sparkleURL];
        NSError *loadError = nil;

        if (sparkleBundle == nil || ![sparkleBundle loadAndReturnError:&loadError]) {
            NSLog(@"[FanControl] Sparkle framework load failed: %@", loadError.localizedDescription);
            return false;
        }

        Class controllerClass = NSClassFromString(@"SPUStandardUpdaterController");
        SEL initializer = NSSelectorFromString(
            @"initWithStartingUpdater:updaterDelegate:userDriverDelegate:"
        );
        id allocatedController = [controllerClass alloc];
        if (controllerClass == Nil || ![allocatedController respondsToSelector:initializer]) {
            NSLog(@"[FanControl] Sparkle updater controller is unavailable");
            return false;
        }

        userDriverDelegate = [[FanControlSparkleDelegate alloc] init];
        NSMethodSignature *signature = [allocatedController methodSignatureForSelector:initializer];
        NSInvocation *invocation = [NSInvocation invocationWithMethodSignature:signature];
        BOOL startUpdater = YES;
        id updaterDelegate = nil;
        id driverDelegate = userDriverDelegate;
        invocation.selector = initializer;
        invocation.target = allocatedController;
        [invocation setArgument:&startUpdater atIndex:2];
        [invocation setArgument:&updaterDelegate atIndex:3];
        [invocation setArgument:&driverDelegate atIndex:4];
        [invocation invoke];

        __unsafe_unretained id initializedController = nil;
        [invocation getReturnValue:&initializedController];
        updaterController = initializedController;
        updater = invokeObjectGetter(
            updaterController,
            NSSelectorFromString(@"updater")
        );
        return updaterController != nil && updater != nil;
    }
}

bool FanControlCanCheckForUpdates(void) {
    @autoreleasepool {
        if (!FanControlInitializeUpdater()) {
            return false;
        }

        return invokeBooleanGetter(
            updater,
            NSSelectorFromString(@"canCheckForUpdates")
        );
    }
}

void FanControlCheckForUpdates(void) {
    @autoreleasepool {
        if (!FanControlInitializeUpdater()) {
            return;
        }

        SEL checkSelector = NSSelectorFromString(@"checkForUpdates:");
        if (![updaterController respondsToSelector:checkSelector]) {
            return;
        }

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Warc-performSelector-leaks"
        [updaterController performSelector:checkSelector withObject:nil];
#pragma clang diagnostic pop
    }
}
