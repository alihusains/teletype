// Plays a macOS system sound by name, for the dictation start/stop cue and
// its preview.
//
// Uses the sounds that ship in /System/Library/Sounds on every Mac so the
// picker offers real audio with nothing to bundle or keep in sync per release.

#import <AppKit/AppKit.h>

// Returns 0 on success, non-zero when the name is not a playable system sound.
int teletype_play_system_sound(const char *name) {
    if (!name) return 1;
    @autoreleasepool {
        NSString *soundName = [NSString stringWithUTF8String:name];
        if (!soundName) return 1;

        // Resolve through NSBundle rather than building a path by hand: this
        // picks up the user's chosen system sound volume and the "alert sound"
        // preference instead of bypassing them.
        NSString *path = [[NSBundle mainBundle] pathForResource:soundName
                                                          ofType:@"aiff"
                                                     inDirectory:@"/System/Library/Sounds"];
        if (!path) return 2;

        NSSound *sound = [[NSSound alloc] initWithContentsOfFile:path byReference:YES];
        if (!sound) return 3;

        // -1 keeps the cue audible when the app is not frontmost, which is the
        // whole point of an audio cue: the user looks at the target app.
        if (![sound play]) return 4;
        return 0;
    }
}