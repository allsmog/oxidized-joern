#define RET unsigned long
const RET f(const RET x);
const RET g(const RET x){return f(x);}
