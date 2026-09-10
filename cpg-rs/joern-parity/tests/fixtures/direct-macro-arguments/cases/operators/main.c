#define APPLY(op,a,b) ((a) op (b))
int plus(int a,int b) {return APPLY(+,a,b); }
int minus(int a,int b) {return APPLY(-,a,b); }
int bitand(int a,int b) {return APPLY(&,a,b); }
