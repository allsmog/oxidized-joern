#define APPLY(op,a,b) ((a) op (b))
int right(int x,int y) { return APPLY(>>,x,-y); }
int left(int x,int y) { return APPLY(<<,x,y); }
