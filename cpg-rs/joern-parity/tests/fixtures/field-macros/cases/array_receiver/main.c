struct Item { int len; int code; };
#define Len len
#define Code code
#define READ(p, i) ((p)[i].Len + (p)[i].Code)
int read(struct Item *p, int i) { return READ(p, i); }
