signed long long int identity(signed long long int value) { signed long long int local = value; return local; }
signed long long int declared(signed long long int value);
signed long long int call_definition(signed long long int value) { return identity(value); }
signed long long int call_prototype(signed long long int value) { return declared(value); }
