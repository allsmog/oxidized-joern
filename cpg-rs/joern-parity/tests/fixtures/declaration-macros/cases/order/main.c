#define RET int
RET first(RET x) {return x;}
#undef RET
#define RET long
RET second(RET x) {return x;}
#undef RET
RET unknown(RET x) {return x;}
