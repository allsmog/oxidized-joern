#define APPLY(op,a,b) ((a) op (b))
int nested(int a,int b,int c) {return APPLY(+, (a,b), c); }
