#define RET int
RET before(int x) {
#undef RET
#define RET long
 return x;
}
RET after(int x) { return x; }
