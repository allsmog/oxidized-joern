#define APPLY(a,op,b) ((a) op (b))
#define UNARY(op,x) (op (x))
int slots(int a,int b) { return APPLY(a,|,b) + UNARY(~,a); }
