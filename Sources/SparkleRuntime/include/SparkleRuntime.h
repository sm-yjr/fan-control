#ifndef SPARKLE_RUNTIME_H
#define SPARKLE_RUNTIME_H

#include <stdbool.h>

bool FanControlInitializeUpdater(void);
bool FanControlCanCheckForUpdates(void);
void FanControlCheckForUpdates(void);

#endif
