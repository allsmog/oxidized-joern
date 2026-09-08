typedef int T;
int callback(int x);
int value(int x) { return (callback)(x); }
