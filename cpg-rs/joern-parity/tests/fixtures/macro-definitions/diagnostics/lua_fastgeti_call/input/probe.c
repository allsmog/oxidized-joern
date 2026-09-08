#include "defs.h"
int probe(int slot) { return slot; }
int invoke(Table *table, int key) { int *slot; return luaV_fastgeti(0, table, key, slot); }
