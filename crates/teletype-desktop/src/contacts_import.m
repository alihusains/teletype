// macOS Contacts import bridge.
//
// Exposes a single C function that requests Contacts access (TCC), fetches all
// contacts via CNContactStore, and returns a malloc'd UTF-8 JSON array of
// objects with keys: givenName, familyName, nickname (null when absent),
// organization (null when absent).
//
// Returns NULL on denial or error. The caller (Rust) frees the returned
// pointer via the same `free` from stdlib (the buffer is malloc'd here).
//
// The TCC prompt + CNContactStore call must run on the main thread (TCC
// requirement). The Rust side dispatches to the main thread before calling.

#import <Foundation/Foundation.h>
#import <Contacts/Contacts.h>
#import <stdlib.h>

// Fetch all contacts and return a malloc'd UTF-8 JSON string.
// Returns NULL on denial or error.
//
// The JSON shape is:
// [
//   {"givenName":"John","familyName":"Doe","nickname":null,"organization":"Acme"},
//   ...
// ]
char *teletype_contacts_fetch(void) {
    @autoreleasepool {
        CNContactStore *store = [[CNContactStore alloc] init];

        // Request access with a bounded semaphore wait (10 s timeout).
        __block BOOL granted = NO;
        __block NSError *accessError = nil;
        dispatch_semaphore_t sem = dispatch_semaphore_create(0);

        [store requestAccessForEntityType:CNEntityTypeContacts
                          completionHandler:^(BOOL g, NSError *err) {
            granted = g;
            accessError = err;
            dispatch_semaphore_signal(sem);
        }];

        // Wait up to 10 seconds for the TCC prompt to resolve.
        dispatch_time_t timeout =
            dispatch_time(DISPATCH_TIME_NOW, (int64_t)(10 * NSEC_PER_SEC));
        long result = dispatch_semaphore_wait(sem, timeout);
        if (result != 0) {
            // Timed out — treat as failure.
            return NULL;
        }

        if (!granted || accessError) {
            return NULL;
        }

        // Fetch all contacts with the keys we need.
        NSArray<NSString *> *keys = @[
            CNContactGivenNameKey,
            CNContactFamilyNameKey,
            CNContactNicknameKey,
            CNContactOrganizationNameKey
        ];

        NSError *fetchError = nil;
        NSPredicate *allContacts = [NSPredicate predicateWithValue:YES];
        NSArray<CNContact *> *contacts =
            [store unifiedContactsMatchingPredicate:allContacts
                                          keysToFetch:keys
                                                error:&fetchError];
        if (fetchError) {
            return NULL;
        }

        // Build the JSON array.
        NSMutableArray<NSDictionary *> *array = [NSMutableArray arrayWithCapacity:contacts.count];
        for (CNContact *c in contacts) {
            NSString *given = c.givenName ?: @"";
            NSString *family = c.familyName ?: @"";
            NSString *nickname = c.nickname.length > 0 ? c.nickname : nil;
            NSString *org = c.organizationName.length > 0 ? c.organizationName : nil;

            NSDictionary *obj = @{
                @"givenName": given,
                @"familyName": family,
                @"nickname": nickname ?: [NSNull null],
                @"organization": org ?: [NSNull null]
            };
            [array addObject:obj];
        }

        NSError *jsonError = nil;
        NSData *jsonData =
            [NSJSONSerialization dataWithJSONObject:array options:0 error:&jsonError];
        if (jsonError || !jsonData) {
            return NULL;
        }

        NSString *jsonString = [[NSString alloc] initWithData:jsonData encoding:NSUTF8StringEncoding];
        if (!jsonString) {
            return NULL;
        }

        const char *utf8 = [jsonString UTF8String];
        if (!utf8) {
            return NULL;
        }

        size_t len = strlen(utf8);
        char *buf = (char *)malloc(len + 1);
        if (!buf) {
            return NULL;
        }
        memcpy(buf, utf8, len + 1);
        return buf;
    }
}

// Free a buffer returned by teletype_contacts_fetch.
// The Rust side can call `free` directly, but we expose this for symmetry
// with other bridges that may use a custom allocator.
void teletype_contacts_free(char *ptr) {
    free(ptr);
}
