package io.github.smallgearlab.sortition;

/** The format of structured data for {@link Lottery#drawData}. */
public enum DataFormat {
    /** JSON. */
    JSON(0),
    /** YAML (aliases, custom tags and multiple documents are rejected). */
    YAML(1),
    /** TOML (it needs a path, because its root is a table). */
    TOML(2);

    final int value;

    DataFormat(int value) {
        this.value = value;
    }
}
