typedef int T;
int (*T)(int);
int value(int x){return (T)(x);}
