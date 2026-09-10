#define RET int
#if 0
#undef RET
#define RET long
RET hidden(int x) { return x; }
#endif
