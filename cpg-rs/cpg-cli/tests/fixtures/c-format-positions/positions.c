char *getenv(const char *name);
int printf(const char *format, ...);
int fprintf(void *stream, const char *format, ...);
void syslog(int priority, const char *format, ...);
int fflush(void *stream);

void printf_format(void) {
    char *value = getenv("FORMAT");
    printf(value);
}

void printf_data(void) {
    char *value = getenv("DATA");
    printf("%s", value);
}

void fprintf_format(void *stream) {
    char *value = getenv("FORMAT");
    fprintf(stream, value);
}

void fprintf_data(void *stream) {
    char *value = getenv("DATA");
    fprintf(stream, "%s", value);
}

void fprintf_stream(void) {
    void *stream = getenv("STREAM");
    fprintf(stream, "%s", "safe");
}

void syslog_format(void) {
    char *value = getenv("FORMAT");
    syslog(3, value);
}

void syslog_data(void) {
    char *value = getenv("DATA");
    syslog(3, "%s", value);
}

void killed_format(void *stream) {
    char *value = getenv("FORMAT");
    value = "%s";
    fprintf(stream, value, "safe");
}

void nested_data(void *stream) {
    fprintf(stream, "%s", getenv("DATA"));
}

#define REPORT(s,p) (fprintf(stream, (s), (p)), fflush(stream))

void macro_format(void *stream) {
    char *value = getenv("FORMAT");
    REPORT(value, "safe");
}

void macro_data(void *stream) {
    char *value = getenv("DATA");
    REPORT("%s", value);
}
