typedef volatile int V;
typedef const int C;
int value(int x){return (V)(x)+(C)(x);}
