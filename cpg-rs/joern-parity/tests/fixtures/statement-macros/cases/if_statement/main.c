#define SET(p,v) if(v) { *(p)=(v); } else { *(p)=0; }
void f(int *p, int v) { SET(p,v); }
