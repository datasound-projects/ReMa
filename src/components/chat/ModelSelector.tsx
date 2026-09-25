import { useNavigation } from '../../app/navigation';
import { sameModel, type ModelCatalog, type ModelRef } from '../../services/providerService';
import { CheckIcon, ChevronDownIcon } from '../icons';
import { Menu } from '../ui/Menu';

interface ModelSelectorProps {
  catalog: ModelCatalog | null;
  value: ModelRef | null;
  onChange: (model: ModelRef) => void;
}

/** Compact model picker for the composer. */
export function ModelSelector({ catalog, value, onChange }: ModelSelectorProps) {
  const { navigate } = useNavigation();
  const models = catalog?.models ?? [];
  const current = models.find((o) => sameModel(o.model, value));

  if (catalog && models.length === 0) {
    return (
      <button type="button" className="model-selector" onClick={() => navigate({ page: 'settings' })}>
        Connect a model
      </button>
    );
  }

  const providers = [...new Set(models.map((o) => o.providerName))];

  return (
    <Menu
      align="start"
      placement="above"
      trigger={(props) => (
        <button type="button" className="model-selector" {...props}>
          <span className="model-selector__name">{current?.displayName ?? 'Choose model'}</span>
          <ChevronDownIcon className="model-selector__chevron" />
        </button>
      )}
    >
      {(close) => (
        <div className="model-menu">
          {providers.map((provider) => (
            <div key={provider} className="model-menu__group">
              <div className="model-menu__label">{provider}</div>
              {models
                .filter((o) => o.providerName === provider)
                .map((option) => {
                  const selected = sameModel(option.model, value);
                  return (
                    <button
                      key={`${option.model.providerId}/${option.model.modelId}`}
                      type="button"
                      role="menuitemradio"
                      aria-checked={selected}
                      className="menu__item model-menu__item"
                      onClick={() => {
                        close();
                        onChange(option.model);
                      }}
                    >
                      <span>{option.displayName}</span>
                      {selected && <CheckIcon className="model-menu__check" />}
                    </button>
                  );
                })}
            </div>
          ))}
          <div className="model-menu__footer">
            <button
              type="button"
              className="menu__item"
              onClick={() => {
                close();
                navigate({ page: 'settings' });
              }}
            >
              Manage models…
            </button>
          </div>
        </div>
      )}
    </Menu>
  );
}
