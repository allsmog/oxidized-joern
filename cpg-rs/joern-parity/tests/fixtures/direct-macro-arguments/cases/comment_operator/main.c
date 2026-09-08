#define APPLY(op,a,b) ((a) op (b))
int commented(int a,int b) {return APPLY(/* left */ & /* right */, a, b); }
