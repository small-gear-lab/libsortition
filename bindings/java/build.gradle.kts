import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    `java-library`
    // Kotlin is only used by a test that checks the library reads naturally from Kotlin.
    kotlin("jvm") version "2.4.21"
}

group = "io.github.small-gear-lab"
version = "0.1.1"

repositories {
    mavenCentral()
}

dependencies {
    testImplementation(platform("org.junit:junit-bom:6.1.3"))
    testImplementation("org.junit.jupiter:junit-jupiter")
    testRuntimeOnly("org.junit.platform:junit-platform-launcher")
    testImplementation("com.google.code.gson:gson:2.15.0")
    testImplementation(kotlin("test-junit5"))
}

// The library calls the C ABI through java.lang.foreign (final since Java 22); Java 25 is the LTS.
tasks.withType<JavaCompile>().configureEach {
    options.release = 25
    options.compilerArgs.addAll(listOf("-Xlint:all,-restricted", "-Werror"))
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_25
        allWarningsAsErrors = true
    }
}

java {
    withSourcesJar()
    withJavadocJar()
}

tasks.javadoc {
    (options as StandardJavadocDocletOptions).addBooleanOption("Xdoclint:all", true)
}

tasks.test {
    useJUnitPlatform()
    jvmArgs("--enable-native-access=ALL-UNNAMED")
    testLogging {
        events("failed")
        exceptionFormat = org.gradle.api.tasks.testing.logging.TestExceptionFormat.FULL
    }
}
