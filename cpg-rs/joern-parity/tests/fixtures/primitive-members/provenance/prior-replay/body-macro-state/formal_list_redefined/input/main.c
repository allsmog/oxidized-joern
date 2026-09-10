#define PICK(x) (x)
#if PICK(1)
#undef PICK
#define PICK(x,y) ((x)+(y))
#endif
int choose(int value) { return PICK(value,2); }
