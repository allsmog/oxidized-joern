#define RET unsigned long
RET f(RET x);
RET g(RET x) {return f(x);}
