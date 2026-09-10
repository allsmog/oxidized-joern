struct Item { int values[4]; int len; };
struct Item *éM(int n);
#define M(n) ((struct Item *)0)
#define Field values[1]
int read(int n) { return éM(n)->Field; }
