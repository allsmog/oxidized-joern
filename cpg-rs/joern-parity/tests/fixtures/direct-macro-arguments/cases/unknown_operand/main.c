#define APPLY(op,a,b) ((a) op (b))
int unknown(int a) { return APPLY(+,a,missing); }
