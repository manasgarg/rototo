function register(lint)
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
          id = "payments/noop",
          title = "No-op rule",
          help = "No-op rule used by tests.",
        },
    handler = "check_workspace",
  })
  lint:on({
    stage = "policy",
    entity = "workspace",
    rule = {
          id = "payments/noop",
          title = "No-op rule",
          help = "No-op rule used by tests.",
        },
    handler = "check_workspace",
  })
end

function check_workspace(ctx)
  return {}
end
