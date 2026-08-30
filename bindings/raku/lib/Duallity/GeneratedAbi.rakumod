unit module Duallity::GeneratedAbi;

# Generated from bindings/api.json. Do not edit by hand.
our constant ABI-VERSION is export = 1;
our constant API-REVISION is export = 2;

our enum Status is export (
    OK => 0,
    INVALID-ARGUMENT => 1,
    INVALID-UTF8 => 2,
    NULL-POINTER => 3,
    PANIC => 4,
    INCOMPATIBLE-RESOURCE => 5,
    PROVIDER-ERROR => 6,
    LIMIT-EXCEEDED => 7,
);

our enum Algorithm is export (
    STANDARD => 0,
    TRANSPOSITION => 1,
    MERGE-AND-SPLIT => 2,
    DAMERAU-LEVENSHTEIN => 3,
);

our enum WfstKind is export (
    LEVENSHTEIN => 0,
    UNIVERSAL-STANDARD => 1,
    UNIVERSAL-TRANSPOSITION => 2,
    UNIVERSAL-MERGE-AND-SPLIT => 3,
    GENERALIZED-STANDARD => 4,
    GENERALIZED-TRANSPOSITION => 5,
    GENERALIZED-MERGE-AND-SPLIT => 6,
    GENERALIZED-PHONETIC => 7,
    FZF => 8,
);
