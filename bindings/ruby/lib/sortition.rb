# frozen_string_literal: true

require "fiddle"
require "fiddle/import"
require "json"

# libsortition の Ruby バインディング（標準ライブラリの Fiddle のみ）
module Sortition
  extend Fiddle::Importer

  ERRORS = {
    -1 => "invalid argument (k > n?)",
    -2 => "OS random source failed",
    -3 => "text is not valid UTF-8",
    -4 => "parse error",
    -5 => "path not found or not an array",
    -6 => "unsupported format",
    -7 => "internal error"
  }.freeze

  FORMATS = { json: 0, yaml: 1, toml: 2 }.freeze

  # コアが返したエラー。code に SORTITION_ERR_* の値が入る。
  class Error < ArgumentError
    attr_reader :code

    def initialize(code, detail = nil)
      @code = code
      super(detail || ERRORS.fetch(code, "error #{code}"))
    end
  end

  def self.lib_name
    case RbConfig::CONFIG["host_os"]
    when /darwin/ then "libsortition.dylib"
    when /mswin|mingw/ then "sortition.dll"
    else "libsortition.so"
    end
  end

  def self.find_lib
    [
      ENV["LIBSORTITION_PATH"],
      File.expand_path(lib_name, __dir__),
      File.expand_path("../../../target/release/#{lib_name}", __dir__)
    ].compact.find { |p| File.exist?(p) } or raise "libsortition not found; set LIBSORTITION_PATH"
  end

  dlload find_lib
  extern "int sortition_draw(unsigned int, unsigned int, void*, void*)"
  extern "long sortition_draw_text(void*, unsigned long, unsigned int, void*, void*, unsigned long)"
  extern "int sortition_draw_data(int, void*, unsigned long, void*, unsigned int, void*, void*)"
  extern "void sortition_free(void*)"
  extern "const char* sortition_last_error()"
  extern "const char* sortition_version()"

  # 0...n から k 個を重複なしで抽選して Array で返す。seed 指定で再現可能。
  def self.draw(n, k, seed: nil)
    out = Fiddle::Pointer.malloc(4 * k, Fiddle::RUBY_FREE)
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    rc = sortition_draw(n, k, seed_ptr, out)
    raise Error.new(rc) unless rc.zero?

    out[0, 4 * k].unpack("L#{k}")
  end

  # 1 行 1 候補のテキストから k 個を重複なしで抽選し、選ばれた候補の Array<String> を返す。
  # 各行は前後の空白を除き、空行は候補に含めない。同じ文字列の行は別の候補として数える。
  def self.draw_text(text, k, seed: nil)
    data = text.encode("UTF-8").b
    out = Fiddle::Pointer.malloc(data.bytesize + 1, Fiddle::RUBY_FREE)
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    n = sortition_draw_text(data, data.bytesize, k, seed_ptr, out, data.bytesize + 1)
    raise Error.new(n) if n.negative?

    n.zero? ? [] : out[0, n].force_encoding("UTF-8").split("\n")
  end

  # JSON / YAML / TOML のテキストにある配列から k 個を重複なしで抽選し、選ばれた要素を返す。
  #
  # format は :json | :yaml | :toml。path は配列の場所（"a.b.0" のドット区切り）で、
  # 空ならルートが配列であること。TOML はルートがテーブルなので path が必要。
  # 要素は JSON に正規化される（TOML の日時は文字列。YAML のエイリアス・独自タグ・
  # 複数ドキュメントは Sortition::Error）。
  def self.draw_data(text, k, format:, path: "", seed: nil)
    fmt = FORMATS.fetch(format.to_sym) { raise ArgumentError, "format must be one of #{FORMATS.keys}" }
    data = text.encode("UTF-8").b
    seed_ptr = seed && Fiddle::Pointer[[seed].pack("Q")]
    out = Fiddle::Pointer.malloc(Fiddle::SIZEOF_VOIDP, Fiddle::RUBY_FREE)
    out[0, Fiddle::SIZEOF_VOIDP] = "\0" * Fiddle::SIZEOF_VOIDP
    rc = sortition_draw_data(fmt, data, data.bytesize, "#{path}\0".b, k, seed_ptr, out)
    raise Error.new(rc, sortition_last_error.to_s.dup.force_encoding("UTF-8")) unless rc.zero?

    addr = out[0, Fiddle::SIZEOF_VOIDP].unpack1("J")
    begin
      JSON.parse(Fiddle::Pointer.new(addr).to_s.force_encoding("UTF-8"))
    ensure
      sortition_free(Fiddle::Pointer.new(addr))
    end
  end

  def self.version = sortition_version.to_s
end
