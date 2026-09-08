struct Item { int len; int code; };
#define Len len
#define Code code
int read(struct Item *p, int i) { return p[i].Len + p[i].Code; }
