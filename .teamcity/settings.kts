import jetbrains.buildServer.configs.kotlin.*
import jetbrains.buildServer.configs.kotlin.buildFeatures.perfmon

version = "2025.11"

project {
    description = "AIO IDEA 宿主：构建服务端与 Web 资产，装入 Compose 运行时并以原子切换发布；端口门户保证站点仅内网可达。"
    buildType(AioIdeaDeploy)
}

object AioIdeaDeploy : BuildType({
    id("AioIdea_Deploy")
    name = "Deploy aio.addzero.site"
    description = "cargo 测试/检查 + zigbuild 服务端 + dx Web 资产 + Compose 原子发布；不构建镜像，运行时复用已发布的 Playwright 镜像。"
    maxRunningBuilds = 1

    vcs {
        root(DslContext.settingsRoot)
        checkoutMode = CheckoutMode.ON_AGENT
        cleanCheckout = true
        checkoutDir = "aio-idea"
    }

    params {
        // 唯一默认值来源；其他脚本一律引用这些参数，不再写死主机。
        param("env.TEAMCITY_DEPLOY_HOST", "okm182")
        param("env.TEAMCITY_DEPLOY_ROOT", "/root/DockerCompose/aio-idea")
        param("env.TEAMCITY_PUBLIC_HOST", "aio.addzero.site")
        param("env.TEAMCITY_TOOLCHAIN", "/opt/aio-idea-toolchain")
        param("env.TEAMCITY_BUILD_PROFILE", "server")
        param("env.TEAMCITY_KEEP_RELEASES", "5")
        // 目标三元组固定，脚本里的变量名不带主机信息。
        param("env.TEAMCITY_TARGET", "x86_64-unknown-linux-gnu.2.17")
        param("env.CARGO_NET_GIT_FETCH_WITH_CLI", "true")
        param("env.CARGO_TERM_COLOR", "always")
        param("env.NODE_OPTIONS", "--max-old-space-size=4096")
    }

    steps {
        step {
            name = "预检工具链"
            type = "simpleRunner"
            param("script.content", "bash .teamcity/scripts/preflight.sh")
            param("teamcity.step.mode", "default")
            param("use.custom.script", "true")
        }
        step {
            name = "验证与构建"
            type = "simpleRunner"
            param("script.content", "bash .teamcity/scripts/build.sh")
            param("teamcity.step.mode", "default")
            param("use.custom.script", "true")
        }
        step {
            name = "打包发布物"
            type = "simpleRunner"
            param("script.content", "bash .teamcity/scripts/package.sh")
            param("teamcity.step.mode", "default")
            param("use.custom.script", "true")
        }
        step {
            name = "发布并验证"
            type = "simpleRunner"
            param("script.content", "bash .teamcity/scripts/deploy.sh")
            param("teamcity.step.mode", "default")
            param("use.custom.script", "true")
        }
    }

    triggers {
        trigger {
            type = "vcsTrigger"
            param("branchFilter", "+:<default>\n+:main")
            param(
                "triggerRules",
                """
                +:**
                -:**.md
                -:.teamcity/README.md
                """.trimIndent()
            )
        }
    }

    requirements {
        contains("teamcity.agent.name", "okm182")
    }

    features {
        perfmon {}
    }

    artifactRules = "dist/release.tar.gz => aio-idea-release.tar.gz"
})
