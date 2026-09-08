#define BLOCK(x) {typedef int T; x=(T)(x);}
int value(int x){BLOCK(x);return x;}
