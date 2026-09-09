long long int identity(long long int value) { long long int local = value; return local; }
long long int declared(long long int value);
long long int call_definition(long long int value) { return identity(value); }
long long int call_prototype(long long int value) { return declared(value); }
