#ifndef DEFS_H
#define DEFS_H
#if MODE == 1
#define SELECT(value) ((value) + 1)
#else
#define SELECT(value) ((value) + 2)
#endif
#endif
