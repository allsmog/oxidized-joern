void render(char *p, int n) {
#ifndef NO_snprintf
    (void)snprintf(p, n, "%d", n);
#else
    sprintf(p, "%d", n);
#endif
}
