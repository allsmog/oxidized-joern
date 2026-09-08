#define APPLY(op,a,b) ((a) op (b))
#define OUT(a,b) APPLY(+,a,b)
int nested(int a,int b) { return OUT(a,b); }
