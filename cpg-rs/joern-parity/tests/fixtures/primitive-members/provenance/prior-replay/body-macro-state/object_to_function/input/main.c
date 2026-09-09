#define PICK 1
#if PICK
#undef PICK
#define PICK(x,y) ((x)+(y))
#endif
int choose(int value) { return PICK(value,2); }
