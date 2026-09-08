#define RET int
#if 0
RET early(int x) { return x; }
#endif
#undef RET
#define RET long
#if 0
RET late(int x) { return x; }
#endif
