unit module Duallity;

use NativeCall;
need Duallity::GeneratedAbi;
need Vinary::Tree::Interop;

our constant ABI-VERSION is export = Duallity::GeneratedAbi::ABI-VERSION;
our constant API-REVISION is export = Duallity::GeneratedAbi::API-REVISION;
our constant Status is export = Duallity::GeneratedAbi::Status;
our constant OK is export = Duallity::GeneratedAbi::OK;
our constant INVALID-ARGUMENT is export = Duallity::GeneratedAbi::INVALID-ARGUMENT;
our constant INVALID-UTF8 is export = Duallity::GeneratedAbi::INVALID-UTF8;
our constant NULL-POINTER is export = Duallity::GeneratedAbi::NULL-POINTER;
our constant PANIC is export = Duallity::GeneratedAbi::PANIC;
our constant INCOMPATIBLE-RESOURCE is export =
    Duallity::GeneratedAbi::INCOMPATIBLE-RESOURCE;
our constant PROVIDER-ERROR is export = Duallity::GeneratedAbi::PROVIDER-ERROR;
our constant LIMIT-EXCEEDED is export = Duallity::GeneratedAbi::LIMIT-EXCEEDED;

our constant Algorithm is export = Duallity::GeneratedAbi::Algorithm;
our constant STANDARD is export = Duallity::GeneratedAbi::STANDARD;
our constant TRANSPOSITION is export = Duallity::GeneratedAbi::TRANSPOSITION;
our constant MERGE-AND-SPLIT is export = Duallity::GeneratedAbi::MERGE-AND-SPLIT;
our constant DAMERAU-LEVENSHTEIN is export =
    Duallity::GeneratedAbi::DAMERAU-LEVENSHTEIN;

our constant WfstKind is export = Duallity::GeneratedAbi::WfstKind;
our constant LEVENSHTEIN is export = Duallity::GeneratedAbi::LEVENSHTEIN;
our constant UNIVERSAL-STANDARD is export =
    Duallity::GeneratedAbi::UNIVERSAL-STANDARD;
our constant UNIVERSAL-TRANSPOSITION is export =
    Duallity::GeneratedAbi::UNIVERSAL-TRANSPOSITION;
our constant UNIVERSAL-MERGE-AND-SPLIT is export =
    Duallity::GeneratedAbi::UNIVERSAL-MERGE-AND-SPLIT;
our constant GENERALIZED-STANDARD is export =
    Duallity::GeneratedAbi::GENERALIZED-STANDARD;
our constant GENERALIZED-TRANSPOSITION is export =
    Duallity::GeneratedAbi::GENERALIZED-TRANSPOSITION;
our constant GENERALIZED-MERGE-AND-SPLIT is export =
    Duallity::GeneratedAbi::GENERALIZED-MERGE-AND-SPLIT;
our constant GENERALIZED-PHONETIC is export =
    Duallity::GeneratedAbi::GENERALIZED-PHONETIC;
our constant FZF is export = Duallity::GeneratedAbi::FZF;

module InteropAccess {
    use Vinary::Tree::Interop;

    our constant ResourceType = Resource;
    our constant DictionaryType = Dictionary;
    our constant WfstType = Wfst;
    our constant RawResourceType = RawResource;

    our sub adopt(RawResource:D $raw --> Resource:D) { adopt-resource($raw) }
    our sub wrap(Resource:D $resource --> Wfst:D) { wfst($resource, :take) }
}

class X::Duallity is Exception {
    has Status:D $.status is required;
    has Str:D $.operation is required;
    has Str:D $.detail = '';

    method message(--> Str:D) {
        my $base = "duallity operation '$!operation' failed with $!status";
        $!detail.chars ?? "$base: $!detail" !! $base
    }
}

sub abi-version(--> UInt:D) is export {
    Duallity::GeneratedAbi::duallity-abi-version().UInt
}
sub api-revision(--> UInt:D) is export {
    Duallity::GeneratedAbi::duallity-api-revision().UInt
}

sub check-status(Int:D $code, Str:D $operation --> Nil) {
    my $status = Status($code);
    return if $status == OK;
    X::Duallity.new(
        :$status,
        :$operation,
        detail =>
            (try Duallity::GeneratedAbi::duallity-last-error-message()) // '',
    ).throw;
}

multi sub raw-resource(InteropAccess::ResourceType:D $resource
    --> InteropAccess::RawResourceType:D) {
    $resource.raw
}
multi sub raw-resource(InteropAccess::DictionaryType:D $dictionary
    --> InteropAccess::RawResourceType:D) {
    $dictionary.resource.raw
}

sub adopt-wfst(Pointer:D $handle --> InteropAccess::WfstType:D) {
    my $raw = InteropAccess::RawResourceType.new;
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-resource($handle, $raw),
        'wfst-resource',
    );
    InteropAccess::wrap(InteropAccess::adopt($raw))
}

sub wfst(
    Mu:D $dictionary,
    Str:D $query,
    UInt:D :$maximum-distance = 1,
    Algorithm:D :$algorithm = STANDARD,
    WfstKind:D :$kind = LEVENSHTEIN,
    --> InteropAccess::WfstType:D
) is export {
    my $bytes = $query.encode('utf8');
    my Pointer $output .= new;
    my $data = $bytes.elems ?? nativecast(Pointer, $bytes) !! Pointer;
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-new-ref(
            raw-resource($dictionary),
            $data,
            $bytes.elems,
            $maximum-distance,
            $algorithm,
            $kind,
            $output,
        ),
        'wfst-new',
    );
    LEAVE Duallity::GeneratedAbi::duallity-wfst-free($output);
    adopt-wfst($output)
}

INIT {
    die "duallity ABI mismatch: native {abi-version()} / facade {ABI-VERSION}"
        unless abi-version() == ABI-VERSION;
    die "duallity API revision {api-revision()} is older than {API-REVISION}"
        unless api-revision() >= API-REVISION;
}
