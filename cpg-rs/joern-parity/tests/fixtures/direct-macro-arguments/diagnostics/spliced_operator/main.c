#define APPLY(op,a,b) ((a) op (b))
int spliced(int a,int b) { return APPLY(>\
>, a, b); }
