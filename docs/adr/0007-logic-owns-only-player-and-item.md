# Logic owns only Player and Item

The migrated Logic package owns Player and Item authority only. Product behavior that requires Hero, Skin, Level, Task, WorldBoss, or another gameplay module is excluded from XKK instead of pulling partial dependencies across the module seam or introducing placeholder implementations.
